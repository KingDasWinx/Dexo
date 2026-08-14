use dexo_driver_api::{
    DdlExecutor, DdlOutcome, DdlPlan, DriverError, GrantRecord, QualifiedName, SecurityAdmin,
};

use crate::error::map_error;
use crate::session::PostgresSession;

#[async_trait::async_trait]
impl DdlExecutor for PostgresSession {
    async fn apply_ddl(&self, plan: &DdlPlan) -> Result<DdlOutcome, DriverError> {
        apply_ddl(self, plan).await
    }
}

pub async fn apply_ddl(
    session: &PostgresSession,
    plan: &DdlPlan,
) -> Result<DdlOutcome, DriverError> {
    if plan.transactional {
        session
            .client
            .batch_execute("BEGIN")
            .await
            .map_err(map_error)?;
        for statement in &plan.statements {
            if let Err(error) = session.client.batch_execute(&statement.sql).await {
                let _ = session.client.batch_execute("ROLLBACK").await;
                let _ = error;
                return Ok(DdlOutcome::RolledBack);
            }
        }
        return match session.client.batch_execute("COMMIT").await {
            Ok(()) => Ok(DdlOutcome::Committed),
            Err(_) => Ok(DdlOutcome::Unknown),
        };
    }
    let mut committed = 0usize;
    for statement in &plan.statements {
        match session.client.batch_execute(&statement.sql).await {
            Ok(()) => committed += 1,
            Err(_) if committed == 0 => return Ok(DdlOutcome::RolledBack),
            Err(_) => return Ok(DdlOutcome::PartiallyCommitted { committed }),
        }
    }
    Ok(DdlOutcome::Committed)
}

#[async_trait::async_trait]
impl SecurityAdmin for PostgresSession {
    async fn list_grants(
        &self,
        principal: Option<&QualifiedName>,
    ) -> Result<Vec<GrantRecord>, DriverError> {
        let sql = "SELECT grantee::text, table_catalog::text, table_schema::text, table_name::text, privilege_type::text
                   FROM information_schema.role_table_grants
                   WHERE ($1::text IS NULL OR grantee = $1)
                   ORDER BY grantee, table_schema, table_name, privilege_type";
        let name = principal.map(QualifiedName::object);
        let rows = self.client.query(sql, &[&name]).await.map_err(map_error)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let grantee: String = row.get(0);
                let catalog: String = row.get(1);
                let schema: String = row.get(2);
                let table: String = row.get(3);
                let privilege: String = row.get(4);
                GrantRecord {
                    principal: QualifiedName::new(None::<String>, None::<String>, grantee),
                    target: QualifiedName::new(Some(catalog), Some(schema), table),
                    privileges: vec![privilege],
                }
            })
            .collect())
    }

    async fn effective_privileges(
        &self,
        principal: &QualifiedName,
        object: &QualifiedName,
    ) -> Result<Vec<String>, DriverError> {
        let rel = match object.schema() {
            Some(schema) => format!("{schema}.{}", object.object()),
            None => object.object().to_string(),
        };
        let checks = ["SELECT", "INSERT", "UPDATE", "DELETE"];
        let mut out = Vec::new();
        for privilege in checks {
            let row = self
                .client
                .query_one(
                    "SELECT has_table_privilege($1, $2, $3)",
                    &[&principal.object(), &rel, &privilege],
                )
                .await
                .map_err(map_error)?;
            if row.get::<_, bool>(0) {
                out.push(privilege.to_string());
            }
        }
        Ok(out)
    }

    async fn set_password(
        &self,
        principal: &QualifiedName,
        password: &secrecy::SecretString,
    ) -> Result<(), DriverError> {
        use secrecy::ExposeSecret;
        // ponytail: PG rejects PASSWORD $1; bind is not valid for ALTER ROLE. Escape for protocol only — never preview/history/SQLite.
        let sql = format!(
            "ALTER ROLE {} PASSWORD '{}'",
            crate::ddl::render::PgDialect::quote_ident(principal.object()),
            password.expose_secret().replace('\'', "''")
        );
        self.client.batch_execute(&sql).await.map_err(map_error)
    }
}
