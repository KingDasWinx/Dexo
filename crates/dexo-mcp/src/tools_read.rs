use std::time::Instant;

use dexo_app::AppError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::error::app_error;
use crate::render::{RowsPage, rows_result, text_result};
use crate::router::{ConnectionSlot, SessionLease};
use crate::schema::SqlInput;
use crate::server::DexoMcpServer;

impl DexoMcpServer {
    pub(crate) fn slot(&self, connection: Option<&str>) -> Result<&ConnectionSlot, CallToolResult> {
        self.inner
            .router
            .resolve(connection)
            .map_err(|error| app_error(&error))
    }

    pub(crate) async fn open(
        &self,
        connection: Option<&str>,
    ) -> Result<SessionLease<'_>, CallToolResult> {
        let slot = self.slot(connection)?;
        self.inner
            .router
            .lease(slot)
            .await
            .map_err(|error| app_error(&error))
    }
}

/// Turns an app result into a tool result, first dropping the session if the error says
/// the connection is gone.
pub(crate) fn finish(
    lease: &mut SessionLease<'_>,
    outcome: Result<CallToolResult, AppError>,
) -> CallToolResult {
    match outcome {
        Ok(result) => result,
        Err(error) => {
            lease.discard_if_broken(&error);
            app_error(&error)
        }
    }
}

#[tool_router(router = read_tools, vis = "pub(crate)")]
impl DexoMcpServer {
    /// The saved connections this profile may use: driver, database, environment, and whether writes are possible at all. Pass a name as `connection` to the other tools.
    #[tool(annotations(read_only_hint = true))]
    async fn list_connections(&self) -> CallToolResult {
        let rows = self
            .inner
            .router
            .connections()
            .map(|connection| {
                vec![
                    connection.name.clone(),
                    connection.driver.clone(),
                    connection.database.clone().unwrap_or_default(),
                    format!("{:?}", connection.environment).to_lowercase(),
                    match connection.accepts_writes() {
                        Ok(()) => "with a grant".to_string(),
                        Err(error) => error.to_string(),
                    },
                ]
            })
            .collect();
        let columns = ["name", "driver", "database", "environment", "writes"]
            .map(String::from)
            .to_vec();
        rows_result(&RowsPage::new(columns, rows))
    }

    /// Check whether query_execute_read would accept a statement, without running it. Says why when it would not.
    #[tool(annotations(read_only_hint = true))]
    async fn query_validate(&self, Parameters(input): Parameters<SqlInput>) -> CallToolResult {
        let slot = match self.slot(input.connection.as_deref()) {
            Ok(slot) => slot,
            Err(result) => return result,
        };
        match self.inner.service.validate_sql(&slot.meta, &input.sql) {
            Ok(()) => text_result(
                "ok: one read-only statement, and every table it names is allowed",
                json!({ "valid": true }),
            ),
            Err(error) => app_error(&error),
        }
    }

    /// Run one read-only statement inside a read-only transaction and return the rows as a table. Every table it names must be inside the profile's allowlist.
    #[tool(annotations(read_only_hint = true))]
    async fn query_execute_read(
        &self,
        Parameters(input): Parameters<SqlInput>,
        cancel: CancellationToken,
    ) -> CallToolResult {
        let slot = match self.slot(input.connection.as_deref()) {
            Ok(slot) => slot,
            Err(result) => return result,
        };
        if let Err(error) = self.inner.service.validate_sql(&slot.meta, &input.sql) {
            return app_error(&error);
        }
        let mut lease = match self.inner.router.lease(slot).await {
            Ok(lease) => lease,
            Err(error) => return app_error(&error),
        };
        let started = Instant::now();
        let outcome = self
            .inner
            .service
            .execute_read(lease.session(), lease.meta, &input.sql, &cancel)
            .await
            .map(|result| {
                rows_result(&RowsPage {
                    columns: result.columns,
                    rows: result.rows,
                    truncated: result.truncated,
                    bytes: result.bytes,
                    elapsed: started.elapsed(),
                    next_offset: None,
                    title: None,
                })
            });
        finish(&mut lease, outcome)
    }

    /// Estimated plan for one read-only statement. Runs EXPLAIN without ANALYZE, so nothing is executed.
    #[tool(annotations(read_only_hint = true))]
    async fn query_explain(&self, Parameters(input): Parameters<SqlInput>) -> CallToolResult {
        let slot = match self.slot(input.connection.as_deref()) {
            Ok(slot) => slot,
            Err(result) => return result,
        };
        if let Err(error) = self.inner.service.validate_sql(&slot.meta, &input.sql) {
            return app_error(&error);
        }
        let mut lease = match self.inner.router.lease(slot).await {
            Ok(lease) => lease,
            Err(error) => return app_error(&error),
        };
        let outcome = self
            .inner
            .service
            .explain(lease.session(), lease.meta, &input.sql)
            .await
            .map(|plan| {
                text_result(
                    serde_json::to_string_pretty(&plan).unwrap_or_default(),
                    serde_json::to_value(&plan).unwrap_or_default(),
                )
            });
        finish(&mut lease, outcome)
    }
}
