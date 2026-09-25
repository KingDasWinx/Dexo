use std::time::Duration;

use dexo_driver_api::{
    CatalogObject, ExplainPlan, ExplainRequest, QueryEvent, QueryRequest, Session, TransactionMode,
};
use dexo_sql::{GuardRejection, StatementEffect, inspect_read, split_statements};
use futures_util::StreamExt;
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::data::display_value;
use crate::error::{AppError, ErrorCategory};
use crate::mcp::connection::McpConnection;
use crate::mcp::policy::{Decision, ObjectPolicy};
use crate::mcp::profile::{McpProfile, QueryMode};
use crate::mcp::selector::ObjectRef;
use crate::query_service::map_driver_error;
use crate::search_service::SearchService;

const HIDDEN: &str = "not found";

/// Rows as the user would see them in the grid, with the limits that cut them short.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ReadResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
    pub bytes: u64,
}

pub struct McpService {
    pub profile: McpProfile,
    objects: Vec<CatalogObject>,
}

impl McpService {
    pub fn new(profile: McpProfile, objects: Vec<CatalogObject>) -> Self {
        let policy = ObjectPolicy::new(profile.selectors.clone());
        let objects = objects
            .into_iter()
            .filter(|object| {
                policy.decide(&ObjectRef::from_catalog_object(object)) == Decision::Allow
            })
            .collect();
        Self { profile, objects }
    }

    pub fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.profile.name,
            "enabled": self.profile.enabled,
            "persistent_access": "read_only",
            "query_mode": format!("{:?}", self.profile.query_mode),
            "limits": self.profile.limits,
            "tools": advertised_tools(&self.profile),
        })
    }

    pub fn search(&self, query: &str) -> Vec<CatalogObject> {
        if query.trim().is_empty() {
            return self.objects.clone();
        }
        SearchService::from_objects(self.objects.clone())
            .search(query)
            .into_iter()
            .map(|hit| hit.object)
            .collect()
    }

    pub fn describe(&self, id_or_name: &str) -> Result<CatalogObject, AppError> {
        self.find(id_or_name).cloned().ok_or_else(hidden)
    }

    pub fn ddl(&self, id_or_name: &str) -> Result<String, AppError> {
        let object = self.describe(id_or_name)?;
        Ok(object
            .attributes
            .get("ddl")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("-- {}", object.qualified_name.display_unquoted())))
    }

    pub fn relationships(&self, id_or_name: &str) -> Result<Vec<CatalogObject>, AppError> {
        let object = self.describe(id_or_name)?;
        Ok(self
            .objects
            .iter()
            .filter(|other| other.parent.as_ref() == Some(&object.id) || other.id == object.id)
            .cloned()
            .collect())
    }

    pub fn authorize_write_sql(&self, sql: &str) -> Result<(), AppError> {
        let spans = split_statements(sql);
        if spans.len() != 1 || !spans[0].understood {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "statement effect is not understood",
            ));
        }
        if spans[0].effect != StatementEffect::DataWrite {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "statement is outside grant capability",
            ));
        }
        Ok(())
    }

    pub fn policy(&self) -> ObjectPolicy {
        ObjectPolicy::new(self.profile.selectors.clone())
    }

    pub fn validate_sql(&self, connection: &McpConnection, sql: &str) -> Result<(), AppError> {
        self.authorize_read_sql(connection, sql)
    }

    pub fn authorize_read_sql(
        &self,
        connection: &McpConnection,
        sql: &str,
    ) -> Result<(), AppError> {
        if self.profile.query_mode != QueryMode::RawReadSql {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "raw SQL is not enabled for this profile",
            ));
        }
        let inspection = inspect_read(sql, connection.dialect).map_err(guard_error)?;
        self.authorize_relations(connection, &inspection.relations)
            .map(|_| ())
    }

    /// Every relation a statement names must be allowed after completing it the way the
    /// server would; one denied or unknown name hides the whole statement.
    pub fn authorize_relations(
        &self,
        connection: &McpConnection,
        relations: &[Vec<String>],
    ) -> Result<Vec<ObjectRef>, AppError> {
        let policy = self.policy();
        let targets: Vec<ObjectRef> = relations
            .iter()
            .map(|path| connection.qualify(path))
            .collect();
        if targets
            .iter()
            .all(|target| policy.decide(target) == Decision::Allow)
        {
            Ok(targets)
        } else {
            Err(hidden())
        }
    }

    /// Runs one authorized read inside `BEGIN READ ONLY … ROLLBACK`. The parse above keeps
    /// out what it can recognise; the transaction is what stops a function that writes.
    pub async fn execute_read(
        &self,
        session: &dyn Session,
        connection: &McpConnection,
        sql: &str,
        cancel: &CancellationToken,
    ) -> Result<ReadResult, AppError> {
        self.authorize_read_sql(connection, sql)?;
        let transactions = session.transactions().ok_or_else(|| {
            AppError::new(
                ErrorCategory::Capability,
                "this connection cannot open a read-only transaction",
            )
        })?;
        transactions
            .begin(TransactionMode::ReadOnly)
            .await
            .map_err(map_driver_error)?;
        let outcome = self.collect_rows(session, sql, cancel).await;
        let rolled_back = transactions.rollback().await.map_err(map_driver_error);
        let result = outcome?;
        rolled_back?;
        Ok(result)
    }

    async fn collect_rows(
        &self,
        session: &dyn Session,
        sql: &str,
        cancel: &CancellationToken,
    ) -> Result<ReadResult, AppError> {
        let limits = &self.profile.limits;
        let mut request = QueryRequest::read(sql, limits.max_rows.saturating_add(1));
        request.timeout = Duration::from_secs(limits.timeout_secs);
        let query = request.id;
        let mut stream = session.execute(request).await.map_err(map_driver_error)?;
        let mut result = ReadResult::default();
        while !result.truncated {
            let event = tokio::select! {
                () = cancel.cancelled() => {
                    let _ = session.cancel(query).await;
                    return Err(AppError::new(ErrorCategory::Cancelled, "cancelled by the client"));
                }
                event = stream.next() => event,
            };
            let Some(event) = event else {
                break;
            };
            match event.map_err(map_driver_error)? {
                QueryEvent::Columns(columns) if result.columns.is_empty() => {
                    result.columns = columns.into_iter().map(|column| column.name).collect();
                }
                QueryEvent::Rows(batch) => {
                    for row in batch.rows {
                        let cells: Vec<String> = row.iter().map(display_value).collect();
                        let size: u64 = cells.iter().map(|cell| cell.len() as u64).sum();
                        if result.rows.len() as u64 >= limits.max_rows
                            || result.bytes + size > limits.max_bytes
                        {
                            result.truncated = true;
                            break;
                        }
                        result.bytes += size;
                        result.rows.push(cells);
                    }
                }
                _ => {}
            }
        }
        Ok(result)
    }

    pub async fn explain(
        &self,
        session: &dyn Session,
        connection: &McpConnection,
        sql: &str,
    ) -> Result<ExplainPlan, AppError> {
        self.authorize_read_sql(connection, sql)?;
        session
            .explain()
            .ok_or_else(|| AppError::new(ErrorCategory::Capability, "explain is unavailable"))?
            .explain(ExplainRequest::estimated(sql))
            .await
            .map_err(map_driver_error)
    }

    fn find(&self, id_or_name: &str) -> Option<&CatalogObject> {
        self.objects.iter().find(|object| {
            object.id.as_str() == id_or_name
                || object.qualified_name.display_unquoted() == id_or_name
                || object.qualified_name.object() == id_or_name
        })
    }
}

pub fn advertised_tools(profile: &McpProfile) -> Vec<&'static str> {
    let mut tools = vec![
        "catalog_search",
        "object_describe",
        "object_get_ddl",
        "object_relationships",
        "query_validate",
        "query_explain",
    ];
    if profile.query_mode == QueryMode::RawReadSql {
        tools.push("query_execute_read");
    }
    tools
        .into_iter()
        .filter(|name| profile.tool_allowed(name))
        .collect()
}

pub fn new_result_uri() -> String {
    format!("dexo://result/{}", Uuid::new_v4())
}

fn hidden() -> AppError {
    AppError::new(ErrorCategory::McpPolicy, HIDDEN)
}

fn guard_error(rejection: GuardRejection) -> AppError {
    AppError::new(ErrorCategory::McpPolicy, rejection.to_string())
}

#[cfg(test)]
mod tests {
    use super::McpConnection;
    use super::McpService;
    use crate::error::ErrorCategory;
    use crate::mcp::profile::{McpProfile, QueryMode};
    use crate::mcp::selector::{Effect, SelectorRule};
    use dexo_driver_api::DbValue;
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    use dexo_test_support::FakeSession;
    use tokio_util::sync::CancellationToken;

    fn table(name: &str) -> CatalogObject {
        CatalogObject::new(
            ObjectId::new(name),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), name),
            None,
        )
    }

    #[test]
    fn denied_objects_are_absent_from_search_and_describe() {
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![
            SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
            SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
        ];
        let service = McpService::new(profile, vec![table("users"), table("secrets")]);
        assert_eq!(service.search("").len(), 1);
        assert!(service.describe("db.public.secrets").is_err());
        assert_eq!(
            service
                .describe("db.public.secrets")
                .unwrap_err()
                .to_string(),
            "not found"
        );
        assert!(service.describe("missing").unwrap_err().to_string() == "not found");
    }

    fn raw_service(max_rows: u64, max_bytes: u64) -> McpService {
        let mut profile = McpProfile::new("assistant");
        profile.query_mode = QueryMode::RawReadSql;
        profile.limits.max_rows = max_rows;
        profile.limits.max_bytes = max_bytes;
        profile.selectors = vec![
            SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
            SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
        ];
        McpService::new(profile, Vec::new())
    }

    fn pg() -> McpConnection {
        McpConnection {
            name: "local".into(),
            driver: "postgres".into(),
            dialect: dexo_sql::Dialect::Postgres,
            database: Some("db".into()),
            default_schema: Some("public".into()),
            environment: crate::connection_policy::Environment::Local,
            read_only: false,
        }
    }

    #[test]
    fn denied_tables_cannot_ride_along_in_a_join() {
        let service = raw_service(10, 1024);
        assert!(service.validate_sql(&pg(), "select * from users").is_ok());
        for sql in [
            "select * from users u join secrets s on true",
            "select * from users, secrets",
            "select * from\nsecrets",
            "select (select 1 from db.public.secrets) from users",
            "WITH x AS (SELECT 1) DELETE FROM users",
            "select * into leaked from users",
        ] {
            assert!(service.validate_sql(&pg(), sql).is_err(), "{sql}");
        }
    }

    #[tokio::test]
    async fn reads_run_in_a_read_only_transaction_that_is_rolled_back() {
        let session = FakeSession::with_rows(&["id"], vec![vec![DbValue::I64(1)]]);
        let result = raw_service(10, 1024)
            .execute_read(
                &session,
                &pg(),
                "select id from users",
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.columns, ["id"]);
        assert_eq!(result.rows, vec![vec!["1".to_string()]]);
        assert_eq!(
            session.log(),
            ["begin ReadOnly", "execute select id from users", "rollback"]
        );
    }

    #[tokio::test]
    async fn rows_past_the_limits_are_cut_and_flagged() {
        let rows = vec![
            vec![DbValue::Text("ab".into())],
            vec![DbValue::Text("cd".into())],
            vec![DbValue::Text("ef".into())],
        ];
        let by_rows = raw_service(2, 1024)
            .execute_read(
                &FakeSession::with_rows(&["v"], rows.clone()),
                &pg(),
                "select v from users",
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(by_rows.rows.len(), 2);
        assert!(by_rows.truncated);
        let by_bytes = raw_service(10, 3)
            .execute_read(
                &FakeSession::with_rows(&["v"], rows),
                &pg(),
                "select v from users",
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(by_bytes.rows.len(), 1);
        assert!(by_bytes.truncated);
    }

    #[tokio::test]
    async fn a_cancelled_read_cancels_the_query_and_rolls_back() {
        let session = FakeSession::hanging();
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            trigger.cancel();
        });
        let error = raw_service(10, 1024)
            .execute_read(&session, &pg(), "select id from users", &cancel)
            .await
            .unwrap_err();
        assert_eq!(error.category(), ErrorCategory::Cancelled);
        assert_eq!(
            session.log(),
            [
                "begin ReadOnly",
                "execute select id from users",
                "cancel",
                "rollback"
            ]
        );
    }
}
