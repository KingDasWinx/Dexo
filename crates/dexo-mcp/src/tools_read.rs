use std::time::Instant;

use dexo_app::mcp::selector::ObjectRef;
use dexo_app::mcp::{Decision, McpService};
use dexo_app::{AppError, CatalogService, ErrorCategory, SearchService, map_driver_error};
use dexo_driver_api::{
    CatalogListOptions, CatalogObject, CatalogReader, ObjectId, ObjectKind, Session,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::error::{app_error, hidden};
use crate::render::{RowsPage, rows_result, text_result};
use crate::router::{ConnectionSlot, SessionLease};
use crate::schema::{CatalogListInput, CatalogSearchInput, DataReadInput, ObjectInput, SqlInput};
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

fn objects_result(objects: &[CatalogObject]) -> CallToolResult {
    let rows = objects
        .iter()
        .map(|object| {
            vec![
                object.id.as_str().to_string(),
                format!("{:?}", object.kind),
                object.qualified_name.display_unquoted(),
            ]
        })
        .collect();
    rows_result(&RowsPage::new(
        ["id", "kind", "name"].map(String::from).to_vec(),
        rows,
    ))
}

fn catalog_of(session: &dyn Session) -> Result<&dyn CatalogReader, AppError> {
    session.catalog().ok_or_else(|| {
        AppError::new(
            ErrorCategory::Capability,
            "this connection has no catalog reader",
        )
    })
}

async fn list(
    service: &McpService,
    session: &dyn Session,
    parent: Option<ObjectId>,
) -> Result<CallToolResult, AppError> {
    let reader = catalog_of(session)?;
    let listed =
        CatalogService::list_children(reader, parent.as_ref(), &CatalogListOptions::default())
            .await?;
    let visible: Vec<CatalogObject> = listed
        .objects
        .into_iter()
        .filter(|object| service.visible(object))
        .collect();
    Ok(objects_result(&visible))
}

/// Finds an object by the name the client typed, completed with the connection's
/// defaults. A denied object and a missing one answer the same way.
async fn find_visible(
    service: &McpService,
    reader: &dyn CatalogReader,
    target: &ObjectRef,
) -> Result<CatalogObject, AppError> {
    if service.policy().decide(target) != Decision::Allow {
        return Err(hidden());
    }
    CatalogService::find_by_qualified_name(
        reader,
        &target.to_string(),
        &CatalogListOptions::default(),
    )
    .await?
    .filter(|object| service.visible(object))
    .ok_or_else(hidden)
}

async fn describe(
    service: &McpService,
    session: &dyn Session,
    target: &ObjectRef,
) -> Result<CallToolResult, AppError> {
    let reader = catalog_of(session)?;
    let object = find_visible(service, reader, target).await?;
    let children =
        CatalogService::list_children(reader, Some(&object.id), &CatalogListOptions::default())
            .await?;
    let keys = match session.data() {
        Some(data) => data
            .table_columns(&object.qualified_name)
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let rows = children
        .objects
        .iter()
        .filter(|child| child.kind == ObjectKind::Column)
        .map(|column| {
            let full = column.qualified_name.object();
            let name = full
                .split_once('.')
                .map_or(full, |(_, name)| name)
                .to_string();
            let role = match keys.iter().find(|key| key.name == name) {
                Some(key) if key.primary_key => "primary key",
                Some(key) if key.unique => "unique",
                _ => "",
            };
            let kind = column
                .attributes
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            vec![name, kind, role.to_string()]
        })
        .collect();
    let mut page = RowsPage::new(["column", "type", "key"].map(String::from).to_vec(), rows);
    page.title = Some(format!(
        "### {} ({:?})",
        object.qualified_name.display_unquoted(),
        object.kind
    ));
    Ok(rows_result(&page))
}

async fn ddl(
    service: &McpService,
    session: &dyn Session,
    target: &ObjectRef,
) -> Result<CallToolResult, AppError> {
    let reader = catalog_of(session)?;
    let object = find_visible(service, reader, target).await?;
    let ddl = CatalogService::ddl(reader, &object.id).await?;
    Ok(text_result(ddl.sql.clone(), json!({ "sql": ddl.sql })))
}

async fn relationships(
    service: &McpService,
    session: &dyn Session,
    target: &ObjectRef,
) -> Result<CallToolResult, AppError> {
    let reader = catalog_of(session)?;
    let object = find_visible(service, reader, target).await?;
    let dependencies = reader
        .dependencies(&object.id)
        .await
        .map_err(map_driver_error)?;
    let dependents = reader
        .dependents(&object.id)
        .await
        .map_err(map_driver_error)?;
    let mut rows = Vec::new();
    for (direction, ids) in [("depends on", dependencies), ("used by", dependents)] {
        for id in ids {
            if let Some(related) = CatalogService::object(reader, &id).await?
                && service.visible(&related)
            {
                rows.push(vec![
                    direction.to_string(),
                    format!("{:?}", related.kind),
                    related.qualified_name.display_unquoted(),
                ]);
            }
        }
    }
    Ok(rows_result(&RowsPage::new(
        ["relation", "kind", "name"].map(String::from).to_vec(),
        rows,
    )))
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

    /// Children of a catalog node, live from the server: databases, then schemas, then tables and views, then columns and indexes. Omit `parent_id` for the roots. Objects outside the allowlist are not listed.
    #[tool(annotations(read_only_hint = true))]
    async fn catalog_list(
        &self,
        Parameters(input): Parameters<CatalogListInput>,
    ) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let parent = input.parent_id.map(ObjectId::new);
        let outcome = list(&self.inner.service, lease.session(), parent).await;
        finish(&mut lease, outcome)
    }

    /// Search table, view and column names in the connection's indexed catalog. The first search on a connection indexes it, which can take a while on a large server.
    #[tool(annotations(read_only_hint = true))]
    async fn catalog_search(
        &self,
        Parameters(input): Parameters<CatalogSearchInput>,
    ) -> CallToolResult {
        let slot = match self.slot(input.connection.as_deref()) {
            Ok(slot) => slot,
            Err(result) => return result,
        };
        let objects = match self
            .inner
            .router
            .backend()
            .catalog_snapshot(&slot.meta.name)
            .await
        {
            Ok(objects) => objects,
            Err(error) => return app_error(&error),
        };
        let visible: Vec<CatalogObject> = objects
            .into_iter()
            .filter(|object| self.inner.service.visible(object))
            .collect();
        let limit = input.limit.unwrap_or(50).clamp(1, 200) as usize;
        let hits: Vec<CatalogObject> = SearchService::from_objects(visible)
            .search(&input.query)
            .into_iter()
            .map(|hit| hit.object)
            .take(limit)
            .collect();
        objects_result(&hits)
    }

    /// Columns (with type and key role) of one table or view.
    #[tool(annotations(read_only_hint = true))]
    async fn object_describe(&self, Parameters(input): Parameters<ObjectInput>) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let target = lease.meta.qualify(&ObjectRef::parse(&input.name).path);
        let outcome = describe(&self.inner.service, lease.session(), &target).await;
        finish(&mut lease, outcome)
    }

    /// The CREATE statement of one object, as the server reports it.
    #[tool(annotations(read_only_hint = true))]
    async fn object_get_ddl(&self, Parameters(input): Parameters<ObjectInput>) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let target = lease.meta.qualify(&ObjectRef::parse(&input.name).path);
        let outcome = ddl(&self.inner.service, lease.session(), &target).await;
        finish(&mut lease, outcome)
    }

    /// Objects this one depends on and objects that depend on it, limited to what the allowlist shows.
    #[tool(annotations(read_only_hint = true))]
    async fn object_relationships(
        &self,
        Parameters(input): Parameters<ObjectInput>,
    ) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let target = lease.meta.qualify(&ObjectRef::parse(&input.name).path);
        let outcome = relationships(&self.inner.service, lease.session(), &target).await;
        finish(&mut lease, outcome)
    }

    /// One page of a table or view, without writing SQL. Works in structured-only profiles. Follow `next_offset` for the next page.
    #[tool(annotations(read_only_hint = true))]
    async fn data_read(
        &self,
        Parameters(input): Parameters<DataReadInput>,
        cancel: CancellationToken,
    ) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let target = lease.meta.qualify(&ObjectRef::parse(&input.table).path);
        let started = Instant::now();
        let outcome = self
            .inner
            .service
            .read_page(
                lease.session(),
                lease.meta,
                &target,
                input.offset.unwrap_or(0),
                input.limit,
                &cancel,
            )
            .await
            .map(|result| {
                rows_result(&RowsPage {
                    columns: result.columns,
                    rows: result.rows,
                    truncated: result.truncated,
                    bytes: result.bytes,
                    elapsed: started.elapsed(),
                    next_offset: result.next_offset,
                    title: None,
                })
            });
        finish(&mut lease, outcome)
    }
}
