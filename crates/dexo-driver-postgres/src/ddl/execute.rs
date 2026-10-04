use dexo_driver_api::{
    DdlExecutor, DdlOutcome, DdlPlan, DriverError, GrantRecord, QualifiedName, SchemaChange,
    SecurityAdmin,
};

use crate::error::map_error;
use crate::session::PostgresSession;

#[async_trait::async_trait]
impl DdlExecutor for PostgresSession {
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
            change_kind(change)
        )));
    }
    Ok(plan)
}

fn change_kind(change: &SchemaChange) -> &'static str {
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
                // The server's reason, as MySQL's first failure gives it: "the change
                // was rolled back" with no why left the user guessing.
                return Err(map_error(error));
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
            Err(error) if committed == 0 => return Err(map_error(error)),
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
        principal: Option<&QualifiedName>,
        object: &QualifiedName,
    ) -> Result<Vec<String>, DriverError> {
        // The relation as the server names it: its parts quoted, so `"Mixed"` is not
        // looked up as `mixed`, and without a schema through the search_path. A
        // function or a type is no relation, and has no table privileges to show.
        let sql = "SELECT p.privilege
                   FROM (SELECT to_regclass(CASE WHEN $2::text IS NULL THEN quote_ident($3)
                                                 ELSE quote_ident($2) || '.' || quote_ident($3)
                                            END) AS oid) r,
                        unnest(ARRAY['SELECT', 'INSERT', 'UPDATE', 'DELETE', 'TRUNCATE',
                                     'REFERENCES', 'TRIGGER'])
                            WITH ORDINALITY AS p(privilege, n)
                   WHERE r.oid IS NOT NULL
                     AND has_table_privilege(COALESCE($1::text, current_user::text)::name,
                                             r.oid, p.privilege)
                   ORDER BY p.n";
        let rows = self
            .client
            .query(
                sql,
                &[
                    &principal.map(QualifiedName::object),
                    &object.schema(),
                    &object.object(),
                ],
            )
            .await
            .map_err(map_error)?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
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
