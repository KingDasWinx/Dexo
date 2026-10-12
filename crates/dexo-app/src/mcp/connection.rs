use dexo_driver_api::{QualifiedName, QueryRequest, Session};
use dexo_sql::Dialect;
use futures_util::StreamExt;

use crate::connection_policy::{ConnectionPolicy, Environment};
use crate::connection_profile::ConnectionProfile;
use crate::error::{AppError, ErrorCategory};
use crate::mcp::selector::ObjectRef;
use crate::query_service::map_driver_error;

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
        let dialect = match dexo_driver_api::DriverDescriptor::family(&profile.driver) {
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
            environment: Environment::parse_strict(&profile.environment),
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

    /// A bare name is judged as `qualify` completes it, and the server resolves it by the
    /// role's search_path: a role whose path starts with `app` read `app.orders` where the
    /// policy had judged `public.orders`. Pinned on a session MCP opens, both mean the same
    /// table for every read, explain and write on it.
    pub async fn pin_search_path(&self, session: &dyn Session) -> Result<(), AppError> {
        let Some(schema) = self
            .default_schema
            .as_deref()
            .filter(|_| self.dialect == Dialect::Postgres)
        else {
            return Ok(());
        };
        let sql = format!("SET search_path = \"{}\"", schema.replace('"', "\"\""));
        let mut stream = session
            .execute(QueryRequest::write(sql))
            .await
            .map_err(map_driver_error)?;
        while let Some(event) = stream.next().await {
            event.map_err(map_driver_error)?;
        }
        Ok(())
    }

    /// Whether `grant` opens anything here: what `accepts_writes` says, and on production
    /// only access given with the connection's name typed -- access given before it was
    /// marked production does not carry over.
    pub fn opens(&self, grant: &crate::mcp::Grant) -> Result<(), AppError> {
        self.accepts_writes()?;
        if self.environment == Environment::Production && !grant.production {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                format!(
                    "{} became a production connection after this write access was given: give it again, typing its name",
                    self.name
                ),
            ));
        }
        Ok(())
    }

    /// MCP never writes to a connection whose own policy keeps it read-only (SEC-011);
    /// no write access overrides that. Production takes writes only through access given
    /// with its name typed: see `opens`.
    pub fn accepts_writes(&self) -> Result<(), AppError> {
        if self.read_only {
            return Err(AppError::new(
                ErrorCategory::Permission,
                "connection is read-only",
            ));
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::McpConnection;
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
}
