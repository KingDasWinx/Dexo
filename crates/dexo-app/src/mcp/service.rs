use std::time::Duration;

use dexo_driver_api::{
    CatalogObject, DataRequest, ExplainPlan, ExplainRequest, ObjectKind, Page, QueryEvent,
    QueryRequest, Session, TransactionMode,
};
use dexo_sql::{GuardRejection, inspect_data_write, inspect_read, inspect_schema_write};
use futures_util::StreamExt;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::data::display_value;
use crate::error::{AppError, ErrorCategory};
use crate::mcp::connection::McpConnection;
use crate::mcp::policy::{Decision, ObjectPolicy};
use crate::mcp::profile::{McpProfile, QueryMode};
use crate::mcp::selector::ObjectRef;
use crate::query_service::map_driver_error;

const HIDDEN: &str = "not found";

/// Rows as the user would see them in the grid, with the limits that cut them short.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ReadResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
    pub bytes: u64,
    pub next_offset: Option<u64>,
}

pub struct McpService {
    pub profile: McpProfile,
}

impl McpService {
    pub fn new(profile: McpProfile) -> Self {
        Self { profile }
    }

    pub fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.profile.name,
            "enabled": self.profile.enabled,
            "persistent_access": format!("{:?}", self.profile.persistent_access),
            "query_mode": format!("{:?}", self.profile.query_mode),
            "limits": self.profile.limits,
            "tools": advertised_tools(&self.profile),
        })
    }

    pub fn data_write_targets(
        &self,
        connection: &McpConnection,
        sql: &str,
    ) -> Result<Vec<ObjectRef>, AppError> {
        let inspection = inspect_data_write(sql, connection.dialect).map_err(guard_error)?;
        self.authorize_relations(connection, &inspection.relations)
    }

    pub fn schema_write_targets(
        &self,
        connection: &McpConnection,
        sql: &str,
    ) -> Result<Vec<ObjectRef>, AppError> {
        let inspection = inspect_schema_write(sql, connection.dialect).map_err(guard_error)?;
        self.authorize_relations(connection, &inspection.relations)
    }

    /// Runs one authorized INSERT/UPDATE/DELETE to completion. The old adapter dropped
    /// the stream unread, so a constraint violation was reported as "sql applied".
    pub async fn execute_write(
        &self,
        session: &dyn Session,
        connection: &McpConnection,
        sql: &str,
    ) -> Result<u64, AppError> {
        self.data_write_targets(connection, sql)?;
        let mut request = QueryRequest::write(sql);
        request.timeout = Duration::from_secs(self.profile.limits.timeout_secs);
        let mut stream = session.execute(request).await.map_err(map_driver_error)?;
        let mut affected = 0;
        while let Some(event) = stream.next().await {
            match event.map_err(map_driver_error)? {
                QueryEvent::ResultSetFinished {
                    rows_affected: Some(rows),
                    ..
                }
                | QueryEvent::Finished {
                    rows_affected: Some(rows),
                } => affected = rows,
                _ => {}
            }
        }
        Ok(affected)
    }

    pub fn visible(&self, object: &CatalogObject) -> bool {
        let reference = ObjectRef::from_catalog_object(object);
        let policy = self.policy();
        match &object.kind {
            ObjectKind::Catalog | ObjectKind::Schema => policy.reveals(&reference),
            _ => policy.decide(&reference) == Decision::Allow,
        }
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

    /// One page of a table through the driver's typed paging, the same path the grid
    /// uses. `next_offset` is set when there is more to read or a limit cut the page.
    pub async fn read_page(
        &self,
        session: &dyn Session,
        connection: &McpConnection,
        target: &ObjectRef,
        offset: u64,
        limit: Option<u32>,
        cancel: &CancellationToken,
    ) -> Result<ReadResult, AppError> {
        if self.policy().decide(target) != Decision::Allow {
            return Err(hidden());
        }
        let data = session.data().ok_or_else(|| {
            AppError::new(
                ErrorCategory::Capability,
                "this connection cannot page table data",
            )
        })?;
        let cap = u32::try_from(self.profile.limits.max_rows)
            .unwrap_or(Page::MAX_LIMIT)
            .min(Page::MAX_LIMIT);
        let page =
            Page::new(offset, limit.unwrap_or(100).clamp(1, cap)).map_err(map_driver_error)?;
        let request = DataRequest {
            object: connection.qualified_name(target),
            columns: Vec::new(),
            filter: None,
            sort: Vec::new(),
            page,
        };
        let fetched = tokio::select! {
            () = cancel.cancelled() => {
                return Err(AppError::new(ErrorCategory::Cancelled, "cancelled by the client"));
            }
            fetched = data.fetch(request) => fetched.map_err(map_driver_error)?,
        };
        let mut result = ReadResult {
            columns: fetched
                .columns
                .into_iter()
                .map(|column| column.name)
                .collect(),
            ..ReadResult::default()
        };
        for row in fetched.rows {
            let cells: Vec<String> = row.iter().map(display_value).collect();
            let size: u64 = cells.iter().map(|cell| cell.len() as u64).sum();
            if result.bytes + size > self.profile.limits.max_bytes {
                result.truncated = true;
                break;
            }
            result.bytes += size;
            result.rows.push(cells);
        }
        result.next_offset =
            (fetched.has_more || result.truncated).then(|| offset + result.rows.len() as u64);
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
}

/// Read tools that exist. Tasks 8–12 add their names here as they land.
pub const READ_TOOLS: &[&str] = &[
    "list_connections",
    "catalog_list",
    "catalog_search",
    "object_describe",
    "object_get_ddl",
    "object_relationships",
    "data_read",
    "query_validate",
    "query_explain",
    "query_execute_read",
    "schema_diff",
];

const RAW_SQL_TOOLS: &[&str] = &["query_validate", "query_explain", "query_execute_read"];

pub fn advertised_tools(profile: &McpProfile) -> Vec<&'static str> {
    READ_TOOLS
        .iter()
        .copied()
        .filter(|name| profile.query_mode == QueryMode::RawReadSql || !RAW_SQL_TOOLS.contains(name))
        .filter(|name| profile.tool_allowed(name))
        .collect()
}

/// Every tool name a profile rule may mention, so a typo is refused instead of silently
/// matching nothing.
pub fn known_tools() -> Vec<&'static str> {
    let mut probe = McpProfile::new("probe");
    probe.query_mode = QueryMode::RawReadSql;
    let mut tools = advertised_tools(&probe);
    tools.extend(crate::mcp::grant::WRITE_TOOLS);
    tools
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
    use dexo_test_support::FakeSession;
    use tokio_util::sync::CancellationToken;

    fn raw_service(max_rows: u64, max_bytes: u64) -> McpService {
        let mut profile = McpProfile::new("assistant");
        profile.query_mode = QueryMode::RawReadSql;
        profile.limits.max_rows = max_rows;
        profile.limits.max_bytes = max_bytes;
        profile.selectors = vec![
            SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
            SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
        ];
        McpService::new(profile)
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
    fn denied_tables_and_their_columns_are_invisible_but_their_schema_is_not() {
        use dexo_driver_api::ObjectKind::{Column, Schema, Table};
        use dexo_driver_api::{CatalogObject, ObjectId, QualifiedName};
        let service = raw_service(10, 1024);
        let object = |kind, name: &str| {
            CatalogObject::new(
                ObjectId::new(name),
                kind,
                QualifiedName::new(Some("db"), Some("public"), name),
                None,
            )
        };
        assert!(service.visible(&object(Schema, "public")));
        assert!(service.visible(&object(Table, "users")));
        assert!(!service.visible(&object(Table, "secrets")));
        assert!(!service.visible(&object(Column, "secrets.email")));
        assert!(!service.visible(&CatalogObject::new(
            ObjectId::new("other"),
            Schema,
            QualifiedName::new(Some("db"), Some("other"), "other"),
            None,
        )));
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
