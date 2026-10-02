use dexo_driver_api::{
    DdlExecutor, DdlOutcome, DdlPlan, DriverError, GrantRecord, QualifiedName, SchemaChange,
    SecurityAdmin,
};
use mysql_async::prelude::Queryable;

use crate::error::map_error;
use crate::session::MysqlSession;

#[async_trait::async_trait]
impl DdlExecutor for MysqlSession {
    fn plan_change(&self, change: &SchemaChange) -> Result<DdlPlan, DriverError> {
        crate::ddl::plan_ddl(change)
    }

    async fn apply_ddl(&self, plan: &DdlPlan) -> Result<DdlOutcome, DriverError> {
        apply_ddl(self, plan).await
    }
}

pub(crate) fn plan_or_unsupported(
    driver: &str,
    change: &SchemaChange,
    plan: DdlPlan,
) -> Result<DdlPlan, DriverError> {
    if plan.statements.is_empty() {
        return Err(DriverError::unsupported(format!(
            "{driver} cannot plan {}",
            match change {
                SchemaChange::CreateTable { .. } => "CreateTable",
                SchemaChange::AlterTable { .. } => "AlterTable",
                SchemaChange::CreateView { .. } => "CreateView",
                SchemaChange::AlterRoutine { .. } => "AlterRoutine",
                SchemaChange::CreateIndex { .. } => "CreateIndex",
                SchemaChange::DropObject { .. } => "DropObject",
                SchemaChange::RenameObject { .. } => "RenameObject",
                SchemaChange::Grant { .. } => "Grant",
                SchemaChange::Revoke { .. } => "Revoke",
            }
        )));
    }
    Ok(plan)
}

pub async fn apply_ddl(session: &MysqlSession, plan: &DdlPlan) -> Result<DdlOutcome, DriverError> {
    let mut committed = 0usize;
    for statement in &plan.statements {
        let mut conn = session.conn.lock().await;
        match conn.query_drop(statement.sql.as_str()).await {
            Ok(()) => committed += 1,
            Err(error) if committed == 0 => return Err(map_error(error)),
            Err(_) => return Ok(DdlOutcome::PartiallyCommitted { committed }),
        }
    }
    Ok(if committed == 0 {
        DdlOutcome::RolledBack
    } else {
        DdlOutcome::Committed
    })
}

/// The privileges a grant can give on one table, in the order they are shown.
const TABLE_PRIVILEGES: &[&str] = &[
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE",
    "DROP",
    "ALTER",
    "INDEX",
    "REFERENCES",
    "TRIGGER",
    "CREATE VIEW",
    "SHOW VIEW",
];

/// `principal` as the LIKE pattern of the `'user'@'host'` information_schema writes,
/// `|` escaping: `user@host` is that account alone, a bare `user` that user at any host.
/// A substring match of the name used to take `dex` for `dexo` too.
fn grantee_like(principal: &str) -> String {
    let part = |text: &str| {
        text.trim_matches(|ch| ch == '\'' || ch == '`')
            .replace('|', "||")
            .replace('%', "|%")
            .replace('_', "|_")
    };
    match principal.rsplit_once('@') {
        Some((user, host)) => format!("'{}'@'{}'", part(user), part(host)),
        None => format!("'{}'@%", part(principal)),
    }
}

#[async_trait::async_trait]
impl SecurityAdmin for MysqlSession {
    async fn list_grants(
        &self,
        principal: Option<&QualifiedName>,
    ) -> Result<Vec<GrantRecord>, DriverError> {
        let sql = "SELECT GRANTEE, TABLE_SCHEMA, TABLE_NAME, PRIVILEGE_TYPE
                   FROM information_schema.TABLE_PRIVILEGES
                   WHERE (? IS NULL OR GRANTEE LIKE ? ESCAPE '|')
                   ORDER BY GRANTEE, TABLE_SCHEMA, TABLE_NAME, PRIVILEGE_TYPE";
        let grantee = principal.map(|principal| grantee_like(principal.object()));
        let mut conn = self.conn.lock().await;
        let rows: Vec<(String, String, String, String)> = conn
            .exec(sql, (&grantee, &grantee))
            .await
            .map_err(map_error)?;
        Ok(rows
            .into_iter()
            .map(|(grantee, schema, table, privilege)| GrantRecord {
                principal: QualifiedName::new(
                    None::<String>,
                    None::<String>,
                    grantee
                        .trim_matches(|ch| ch == '\'' || ch == '`')
                        .to_string(),
                ),
                target: QualifiedName::new(Some(schema), None::<String>, table),
                privileges: vec![privilege],
            })
            .collect())
    }

    async fn effective_privileges(
        &self,
        principal: Option<&QualifiedName>,
        object: &QualifiedName,
    ) -> Result<Vec<String>, DriverError> {
        // A privilege on a table comes from a grant on it, on its schema -- whose name
        // in a grant may hold wildcards, matched the way MySQL matches them -- or on
        // everything: only the first used to count, so root showed nothing.
        // ponytail: privileges that come through an active role are not counted;
        // information_schema lists them under the role, which a plain user cannot see.
        let sql = "SELECT PRIVILEGE_TYPE FROM information_schema.USER_PRIVILEGES
                   WHERE GRANTEE LIKE ? ESCAPE '|'
                   UNION ALL
                   SELECT PRIVILEGE_TYPE FROM information_schema.SCHEMA_PRIVILEGES
                   WHERE GRANTEE LIKE ? ESCAPE '|' AND COALESCE(?, DATABASE()) LIKE TABLE_SCHEMA
                   UNION ALL
                   SELECT PRIVILEGE_TYPE FROM information_schema.TABLE_PRIVILEGES
                   WHERE GRANTEE LIKE ? ESCAPE '|' AND TABLE_SCHEMA = COALESCE(?, DATABASE())
                     AND TABLE_NAME = ?";
        let mut conn = self.conn.lock().await;
        let grantee = match principal {
            Some(principal) => grantee_like(principal.object()),
            None => {
                let current: Option<String> = conn
                    .query_first("SELECT CURRENT_USER()")
                    .await
                    .map_err(map_error)?;
                grantee_like(&current.unwrap_or_default())
            }
        };
        let schema = object.schema().or(object.catalog());
        let found: Vec<String> = conn
            .exec(
                sql,
                (
                    &grantee,
                    &grantee,
                    schema,
                    &grantee,
                    schema,
                    object.object(),
                ),
            )
            .await
            .map_err(map_error)?;
        Ok(TABLE_PRIVILEGES
            .iter()
            .filter(|privilege| found.iter().any(|found| found == *privilege))
            .map(|privilege| privilege.to_string())
            .collect())
    }

    async fn set_password(
        &self,
        principal: &QualifiedName,
        password: &secrecy::SecretString,
    ) -> Result<(), DriverError> {
        use secrecy::ExposeSecret;
        // ponytail: MySQL ALTER USER rejects placeholders for the user ident. Escape for protocol only.
        let sql = format!(
            "ALTER USER {} IDENTIFIED BY {}",
            crate::ddl::render::MysqlDialect::quote_ident(principal.object()),
            dexo_driver_api::mysql_string_literal(password.expose_secret())
        );
        let mut conn = self.conn.lock().await;
        conn.query_drop(sql).await.map_err(map_error)
    }
}

#[cfg(test)]
mod tests {
    use super::grantee_like;

    #[test]
    fn a_grantee_is_matched_whole() {
        assert_eq!(grantee_like("dexo@%"), "'dexo'@'|%'");
        assert_eq!(grantee_like("'app'@'10.0.0.1'"), "'app'@'10.0.0.1'");
        // A bare name is that user at any host, and its wildcards are its own letters.
        assert_eq!(grantee_like("dex"), "'dex'@%");
        assert_eq!(grantee_like("a_b"), "'a|_b'@%");
    }
}
