use dexo_driver_api::{
    DdlExecutor, DdlOutcome, DdlPlan, DriverError, GrantRecord, QualifiedName, SecurityAdmin,
};
use mysql_async::prelude::Queryable;

use crate::error::map_error;
use crate::session::MysqlSession;

#[async_trait::async_trait]
impl DdlExecutor for MysqlSession {
    async fn apply_ddl(&self, plan: &DdlPlan) -> Result<DdlOutcome, DriverError> {
        apply_ddl(self, plan).await
    }
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

#[async_trait::async_trait]
impl SecurityAdmin for MysqlSession {
    async fn list_grants(
        &self,
        principal: Option<&QualifiedName>,
    ) -> Result<Vec<GrantRecord>, DriverError> {
        let sql = "SELECT GRANTEE, TABLE_SCHEMA, TABLE_NAME, PRIVILEGE_TYPE
                   FROM information_schema.TABLE_PRIVILEGES
                   WHERE (? IS NULL OR GRANTEE LIKE CONCAT('%', ?, '%'))
                   ORDER BY GRANTEE, TABLE_SCHEMA, TABLE_NAME, PRIVILEGE_TYPE";
        let name = principal.map(QualifiedName::object);
        let mut conn = self.conn.lock().await;
        let rows: Vec<(String, String, String, String)> =
            conn.exec(sql, (name, name)).await.map_err(map_error)?;
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
        principal: &QualifiedName,
        object: &QualifiedName,
    ) -> Result<Vec<String>, DriverError> {
        let grants = self.list_grants(Some(principal)).await?;
        Ok(grants
            .into_iter()
            .filter(|grant| grant.target.object() == object.object())
            .flat_map(|grant| grant.privileges)
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
            "ALTER USER {} IDENTIFIED BY '{}'",
            crate::ddl::render::MysqlDialect::quote_ident(principal.object()),
            password.expose_secret().replace('\'', "''")
        );
        let mut conn = self.conn.lock().await;
        conn.query_drop(sql).await.map_err(map_error)
    }
}
