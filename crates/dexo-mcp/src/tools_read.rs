use std::time::Instant;

use dexo_app::mcp::selector::ObjectRef;
use dexo_app::mcp::{Decision, McpService};
use dexo_app::schema_diff::{SchemaDifference, plan_migration, render_unquoted};
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
use crate::schema::{
    AdminListInput, CatalogListInput, CatalogSearchInput, DataReadInput, DiffInput, ExplainInput,
    ObjectInput, OverviewInput, SqlInput,
};
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
                object.kind.as_str().to_string(),
                ObjectRef::from_catalog_object(object).to_string(),
            ]
        })
        .collect();
    rows_result(&RowsPage::new(
        ["id", "kind", "name"].map(String::from).to_vec(),
        rows,
    ))
}

/// Each table as a heading, its columns as a table and its constraints as a list, as many
/// as the profile's byte limit holds; the same tables in `structuredContent`.
fn overview_result(overview: &dexo_app::mcp::overview::Overview, max_bytes: u64) -> CallToolResult {
    let columns = ["column", "type", "null", "default"].map(String::from);
    let mut text = String::new();
    let mut shown = 0;
    for table in &overview.tables {
        let rows = match (table.rows, table.estimated_rows) {
            (Some(rows), _) => format!(", {rows} rows"),
            (None, Some(rows)) => format!(", ~{rows} rows"),
            (None, None) => String::new(),
        };
        let mut part = format!("### {} ({}{rows})\n\n", table.name, table.kind);
        let cells: Vec<Vec<String>> = table
            .columns
            .iter()
            .map(|column| {
                vec![
                    column.name.clone(),
                    column.type_name.clone(),
                    if column.nullable { "yes" } else { "no" }.into(),
                    column.default.clone().unwrap_or_default(),
                ]
            })
            .collect();
        part.push_str(&crate::render::markdown_table(&columns, &cells));
        part = part.trim_end().to_string();
        for constraint in &table.constraints {
            part.push_str(&format!("\n- {constraint}"));
        }
        part.push_str("\n\n");
        if shown > 0 && (text.len() + part.len()) as u64 > max_bytes {
            break;
        }
        text.push_str(&part);
        shown += 1;
    }
    if overview.tables.is_empty() {
        text.push_str("No table or view here that this profile may see.");
    }
    if shown < overview.tables.len() || overview.truncated {
        text.push_str(&format!(
            "{shown} of {} tables shown, at the profile's byte limit: name a schema.",
            overview.tables.len()
        ));
    }
    text_result(
        text.trim_end().to_string(),
        json!({
            "tables": &overview.tables[..shown],
            "truncated": shown < overview.tables.len() || overview.truncated,
        }),
    )
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
    let parent = match &parent {
        Some(id) => CatalogService::object(reader, id).await.ok().flatten(),
        None => None,
    };
    let visible: Vec<CatalogObject> = listed
        .objects
        .into_iter()
        .filter(|object| service.visible_under(object, parent.as_ref()))
        .collect();
    Ok(objects_result(&visible))
}

/// Whether `object` may be shown, its relation looked up first: an index or a constraint
/// of a hidden table is hidden with it. A lookup that fails finds no relation, which
/// hides what needs one.
async fn shown(service: &McpService, reader: &dyn CatalogReader, object: &CatalogObject) -> bool {
    let parent = match &object.parent {
        Some(id) => CatalogService::object(reader, id).await.ok().flatten(),
        None => None,
    };
    service.visible_under(object, parent.as_ref())
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
    let object = CatalogService::find_by_qualified_name(
        reader,
        &target.to_string(),
        &CatalogListOptions::default(),
    )
    .await?
    .ok_or_else(hidden)?;
    if shown(service, reader, &object).await {
        Ok(object)
    } else {
        Err(hidden())
    }
}

/// What people say an object is: their note, or else the database's own comment.
pub(crate) fn note_of(
    object: &CatalogObject,
    notes: &std::collections::HashMap<String, String>,
) -> Option<String> {
    notes
        .get(&object.qualified_name.display_unquoted())
        .cloned()
        .or_else(|| {
            object
                .attributes
                .get("comment")
                .and_then(Value::as_str)
                .map(|comment| format!("{comment} (database comment)"))
        })
}

async fn describe(
    service: &McpService,
    session: &dyn Session,
    target: &ObjectRef,
    notes: &std::collections::HashMap<String, String>,
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
            let note = note_of(column, notes).unwrap_or_default();
            vec![name, kind, role.to_string(), note]
        })
        .collect();
    let mut page = RowsPage::new(
        ["column", "type", "key", "note"].map(String::from).to_vec(),
        rows,
    );
    let heading = format!(
        "### {} ({})",
        ObjectRef::from_catalog_object(&object),
        object.kind.as_str()
    );
    page.title = Some(match note_of(&object, notes) {
        Some(note) => format!("{heading}\n\n{note}"),
        None => heading,
    });
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
                && shown(service, reader, &related).await
            {
                rows.push(vec![
                    direction.to_string(),
                    related.kind.as_str().to_string(),
                    ObjectRef::from_catalog_object(&related).to_string(),
                ]);
            }
        }
    }
    Ok(rows_result(&RowsPage::new(
        ["relation", "kind", "name"].map(String::from).to_vec(),
        rows,
    )))
}

async fn sessions(session: &dyn Session) -> Result<CallToolResult, AppError> {
    let admin = session.admin().ok_or_else(|| {
        AppError::new(
            ErrorCategory::Capability,
            "administration is unavailable on this connection",
        )
    })?;
    let list = admin.list_sessions().await.map_err(map_driver_error)?;
    let rows = list
        .items
        .into_iter()
        .map(|item| {
            vec![
                item.id,
                item.user.unwrap_or_default(),
                item.database.unwrap_or_default(),
                item.state,
                item.duration_ms
                    .map(|ms| ms.to_string())
                    .unwrap_or_default(),
            ]
        })
        .collect();
    let mut page = RowsPage::new(
        ["session_id", "user", "database", "state", "duration_ms"]
            .map(String::from)
            .to_vec(),
        rows,
    );
    page.title = list
        .restriction
        .map(|reason| format!("Partial list: {reason}"));
    Ok(rows_result(&page))
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
                        Ok(()) => "while the profile has write access here".to_string(),
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

    /// Estimated plan for one read-only statement. Runs EXPLAIN without ANALYZE, so nothing is executed. `hypothetical_indexes` plans as if those indexes were built (Postgres with hypopg), to see whether one would help before anyone builds it.
    #[tool(annotations(read_only_hint = true))]
    async fn query_explain(&self, Parameters(input): Parameters<ExplainInput>) -> CallToolResult {
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
            .explain(
                lease.session(),
                lease.meta,
                &input.sql,
                &input.hypothetical_indexes,
            )
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

    /// Tables, views, columns, indexes and constraints by name, or by what their notes say, in the connection's indexed catalog. The first search on a connection indexes it, which can take a while on a large server.
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
        let by_id: std::collections::HashMap<&ObjectId, &CatalogObject> =
            objects.iter().map(|object| (&object.id, object)).collect();
        let visible: Vec<CatalogObject> = objects
            .iter()
            .filter(|object| {
                let parent = object.parent.as_ref().and_then(|id| by_id.get(id)).copied();
                self.inner.service.visible_under(object, parent)
            })
            .cloned()
            .collect();
        let notes = self
            .inner
            .router
            .backend()
            .notes(&slot.meta.name)
            .unwrap_or_default();
        let limit = input.limit.unwrap_or(50).clamp(1, 200) as usize;
        let mut hits: Vec<CatalogObject> = SearchService::from_objects(visible.clone())
            .search(&input.query)
            .into_iter()
            .map(|hit| hit.object)
            .take(limit)
            .collect();
        // An object whose note says what was asked for is found by its meaning too --
        // among the visible ones only, so a note never shows what the profile hides.
        let wanted = input.query.trim().to_lowercase();
        if !wanted.is_empty() {
            for object in visible {
                if hits.len() >= limit {
                    break;
                }
                let says = note_of(&object, &notes)
                    .is_some_and(|note| note.to_lowercase().contains(&wanted));
                if says && hits.iter().all(|hit| hit.id != object.id) {
                    hits.push(object);
                }
            }
        }
        let rows = hits
            .iter()
            .map(|object| {
                vec![
                    object.kind.as_str().to_string(),
                    ObjectRef::from_catalog_object(object).to_string(),
                    note_of(object, &notes).unwrap_or_default(),
                ]
            })
            .collect();
        rows_result(&RowsPage::new(
            ["kind", "name", "note"].map(String::from).to_vec(),
            rows,
        ))
    }

    /// Columns (with type and key role) of one table or view.
    #[tool(annotations(read_only_hint = true))]
    async fn object_describe(&self, Parameters(input): Parameters<ObjectInput>) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let target = lease.meta.qualify(&ObjectRef::parse(&input.name).path);
        let notes = self
            .inner
            .router
            .backend()
            .notes(&lease.meta.name)
            .unwrap_or_default();
        let outcome = describe(&self.inner.service, lease.session(), &target, &notes).await;
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

    /// Every table and view of a schema the profile sees, in one call: columns with type, nullability and default, primary and unique keys, foreign keys and checks, and the row count the server estimates -- or counts, with `exact_counts`. Use it instead of object_describe table by table.
    #[tool(annotations(read_only_hint = true))]
    async fn schema_overview(
        &self,
        Parameters(input): Parameters<OverviewInput>,
    ) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let service = &self.inner.service;
        let outcome = service
            .overview(
                lease.session(),
                lease.meta,
                input.schema.as_deref(),
                input.exact_counts.unwrap_or(false),
            )
            .await
            .map(|overview| overview_result(&overview, service.profile.limits.max_bytes));
        finish(&mut lease, outcome)
    }

    /// Differences between two saved schema snapshots, limited to objects this profile may see. Read-only: nothing is applied.
    #[tool(annotations(read_only_hint = true))]
    async fn schema_diff(&self, Parameters(input): Parameters<DiffInput>) -> CallToolResult {
        let connection = match self.inner.router.resolve(input.connection.as_deref()) {
            Ok(slot) => slot.meta.name.clone(),
            Err(error) => return app_error(&error),
        };
        let backend = self.inner.router.backend();
        let load = |name: &str| {
            backend
                .schema_snapshot(name, &connection)
                .and_then(|snapshot| snapshot.ok_or_else(hidden))
        };
        let (from, to) = match (load(&input.from_snapshot), load(&input.to_snapshot)) {
            (Ok(from), Ok(to)) => (from, to),
            (Err(error), _) | (_, Err(error)) => return app_error(&error),
        };
        let (changes, _, _) = plan_migration(&from, &to, &[], render_unquoted);
        let service = &self.inner.service;
        // Each side is judged with its own snapshot, where its relation is.
        let visible_in =
            |object: &CatalogObject, snapshot: &dexo_app::schema_diff::SchemaSnapshot| {
                let parent = object
                    .parent
                    .as_ref()
                    .and_then(|id| snapshot.objects.iter().find(|parent| &parent.id == id));
                service.visible_under(object, parent)
            };
        let rows = changes
            .iter()
            .filter(|change| match change {
                SchemaDifference::Added(object) => visible_in(object, &to),
                SchemaDifference::Removed(object) => visible_in(object, &from),
                SchemaDifference::Changed { before, after } => {
                    visible_in(before, &from) && visible_in(after, &to)
                }
            })
            .map(|change| {
                let kind = match change {
                    SchemaDifference::Added(_) => "added",
                    SchemaDifference::Removed(_) => "removed",
                    SchemaDifference::Changed { .. } => "changed",
                };
                vec![kind.to_string(), change.object_name()]
            })
            .collect();
        rows_result(&RowsPage::new(
            ["change", "object"].map(String::from).to_vec(),
            rows,
        ))
    }

    /// Server sessions: id, user, database, state and duration. Query text is not returned. Listed only when the profile allows it explicitly.
    #[tool(annotations(read_only_hint = true))]
    async fn admin_list_sessions(
        &self,
        Parameters(input): Parameters<AdminListInput>,
    ) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let outcome = sessions(lease.session()).await;
        finish(&mut lease, outcome)
    }
}
