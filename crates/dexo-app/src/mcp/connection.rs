use dexo_driver_api::QualifiedName;
use dexo_sql::Dialect;

use crate::connection_policy::{ConnectionPolicy, Environment};
use crate::connection_profile::ConnectionProfile;
use crate::error::{AppError, ErrorCategory};
use crate::mcp::selector::ObjectRef;

/// What MCP needs to know about one saved connection a profile may use: how to parse its
/// SQL, how to complete a partial name the way the server would, and whether it may be
/// written to at all.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpConnection {
    pub name: String,
    pub driver: String,
    pub dialect: Dialect,
    pub database: Option<String>,
    pub default_schema: Option<String>,
    pub environment: Environment,
    pub read_only: bool,
}

impl McpConnection {
    pub fn from_profile(profile: &ConnectionProfile) -> Result<Self, AppError> {
        let dialect = match profile.driver.as_str() {
            "postgres" => Dialect::Postgres,
            "mysql" => Dialect::Mysql,
            other => {
                return Err(AppError::new(
                    ErrorCategory::Capability,
                    format!("MCP does not support the {other} driver"),
                ));
            }
        };
        let policy = ConnectionPolicy::resolve(&profile.environment, &profile.policy)?;
        let database = ["database", "dbname"]
            .iter()
            .find_map(|key| profile.config.get(*key))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        Ok(Self {
            name: profile.name.clone(),
            driver: profile.driver.clone(),
            dialect,
            database,
            default_schema: (dialect == Dialect::Postgres).then(|| "public".to_string()),
            environment: mcp_environment(&profile.environment),
            read_only: policy.read_only,
        })
    }

    /// Completes a name the way the server resolves it: a bare Postgres table is
    /// `database.public.table`, a bare MySQL table is `database.table`. Without a known
    /// database the name is left as typed, which matches no three-part selector and is
    /// therefore denied.
    pub fn qualify(&self, path: &[String]) -> ObjectRef {
        let Some(database) = &self.database else {
            return ObjectRef {
                path: path.to_vec(),
            };
        };
        let defaults: Vec<String> = std::iter::once(database.clone())
            .chain(self.default_schema.clone())
            .collect();
        let missing = defaults.len().saturating_sub(path.len().saturating_sub(1));
        ObjectRef {
            path: defaults[..missing]
                .iter()
                .cloned()
                .chain(path.iter().cloned())
                .collect(),
        }
    }

    pub fn qualified_name(&self, reference: &ObjectRef) -> QualifiedName {
        let path = &reference.path;
        match (self.dialect, path.as_slice()) {
            (Dialect::Postgres, [catalog, schema, object]) => {
                QualifiedName::new(Some(catalog), Some(schema), object)
            }
            (Dialect::Postgres, [schema, object]) => {
                QualifiedName::new(None::<String>, Some(schema), object)
            }
            (Dialect::Mysql, [catalog, object]) => {
                QualifiedName::new(Some(catalog), None::<String>, object)
            }
            (_, [.., object]) => QualifiedName::new(None::<String>, None::<String>, object),
            (_, []) => QualifiedName::new(None::<String>, None::<String>, ""),
        }
    }
}

/// `Environment::parse` maps any unknown label to `Local`; MCP must fail closed instead,
/// so `prod`, `PRD` or `live` count as production.
fn mcp_environment(label: &str) -> Environment {
    match Environment::known(label) {
        Some(environment) => environment,
        None if label.is_empty() || label.eq_ignore_ascii_case("local") => Environment::Local,
        None => Environment::Production,
    }
}

#[cfg(test)]
mod tests {
    use super::{McpConnection, mcp_environment};
    use crate::connection_policy::Environment;
    use dexo_sql::Dialect;

    fn postgres() -> McpConnection {
        McpConnection {
            name: "local".into(),
            driver: "postgres".into(),
            dialect: Dialect::Postgres,
            database: Some("db".into()),
            default_schema: Some("public".into()),
            environment: Environment::Local,
            read_only: false,
        }
    }

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    #[test]
    fn bare_names_complete_to_the_server_default() {
        let pg = postgres();
        assert_eq!(pg.qualify(&path(&["t"])).path, ["db", "public", "t"]);
        assert_eq!(pg.qualify(&path(&["s", "t"])).path, ["db", "s", "t"]);
        assert_eq!(pg.qualify(&path(&["x", "s", "t"])).path, ["x", "s", "t"]);
        let mysql = McpConnection {
            dialect: Dialect::Mysql,
            default_schema: None,
            ..postgres()
        };
        assert_eq!(mysql.qualify(&path(&["t"])).path, ["db", "t"]);
        assert_eq!(mysql.qualify(&path(&["shop", "t"])).path, ["shop", "t"]);
    }

    #[test]
    fn unknown_environment_labels_fail_closed() {
        assert_eq!(mcp_environment("prod"), Environment::Production);
        assert_eq!(mcp_environment("PRD"), Environment::Production);
        assert_eq!(mcp_environment("local"), Environment::Local);
        assert_eq!(mcp_environment("staging"), Environment::Staging);
    }
}
