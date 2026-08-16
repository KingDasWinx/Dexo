use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dexo_app::schema::{CatalogScope, Confirmation, ConfirmationAnswer};
use dexo_app::schema_diff::{DiffSource, SchemaSnapshot};
use dexo_app::transfer::{RecordingSink, export_row_batches};
use dexo_driver_api::{
    AlterOp, CatalogObject, ColumnSpec, DdlExecutor, DdlOutcome, DdlPlan, DriverError, ObjectId,
    ObjectKind, QualifiedName, SchemaChange, TableDef, TableShape,
};
use dexo_tui::runtime::explain_manager::ExplainManager;
use dexo_tui::runtime::schema_manager::{DiffFilters, DiffRequest, SchemaManager};
use dexo_tui::screens::file_picker::FilePicker;

struct RecordingDdl {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl DdlExecutor for RecordingDdl {
    fn plan_change(&self, change: &SchemaChange) -> Result<DdlPlan, DriverError> {
        let mut plan = DdlPlan {
            risk: change.risk(),
            transactional: true,
            ..DdlPlan::default()
        };
        plan.push(format!("-- {}", change.target().display_unquoted()), false);
        Ok(plan)
    }

    async fn apply_ddl(&self, _: &DdlPlan) -> Result<DdlOutcome, DriverError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(DdlOutcome::Committed)
    }
}

fn session_id() -> String {
    "session-1".into()
}

fn table_id() -> ObjectId {
    ObjectId::new("orders")
}

fn add_column() -> SchemaChange {
    SchemaChange::AlterTable {
        target: QualifiedName::new(Some("db"), Some("public"), "orders"),
        ops: vec![AlterOp::DropColumn {
            name: QualifiedName::new(None::<String>, None::<String>, "qty"),
        }],
    }
}

fn runtime_with_recording_ddl() -> SchemaManager {
    SchemaManager::new(
        Arc::new(RecordingDdl {
            calls: AtomicUsize::new(0),
        }),
        session_id(),
    )
}

#[tokio::test]
async fn confirmed_schema_change_uses_selected_session_and_invalidates_scope() {
    let runtime = runtime_with_recording_ddl();
    let op = runtime
        .preview_schema(&session_id(), add_column())
        .await
        .unwrap();
    assert!(matches!(op.confirmation, Confirmation::TypeTarget(_)));
    runtime
        .apply_schema(
            op.operation_id,
            ConfirmationAnswer::Text("db.public.orders".into()),
        )
        .await
        .unwrap();
    assert_eq!(runtime.ddl_calls(), 1);
    assert_eq!(
        runtime.invalidations(),
        vec![CatalogScope::Table(table_id())]
    );
}

fn snapshot_id() -> String {
    "snap-1".into()
}

fn runtime_with_catalog_and_snapshot() -> SchemaManager {
    let runtime = runtime_with_recording_ddl();
    let orders = CatalogObject::new(
        ObjectId::new("orders"),
        ObjectKind::Table,
        QualifiedName::new(Some("db"), Some("public"), "orders"),
        None,
    );
    let saved = SchemaSnapshot::capture("postgres", "16", "2026-08-01T00:00:00Z", "db", vec![]);
    let live = SchemaSnapshot::capture(
        "postgres",
        "16",
        "2026-08-15T00:00:00Z",
        "db",
        vec![
            orders.clone(),
            CatalogObject::new(
                ObjectId::new("items"),
                ObjectKind::Table,
                QualifiedName::new(Some("db"), Some("public"), "items"),
                None,
            ),
        ],
    );
    runtime.put_snapshot(snapshot_id(), saved);
    runtime.put_live(session_id(), live);
    runtime
}

#[tokio::test]
async fn diff_loads_both_selected_sources_instead_of_fixture_objects() {
    let runtime = runtime_with_catalog_and_snapshot();
    let result = runtime
        .diff(DiffRequest {
            left: DiffSource::SavedSnapshot(snapshot_id()),
            right: DiffSource::Live(session_id()),
            filters: DiffFilters::all(),
            renames: vec![],
        })
        .await
        .unwrap();
    assert!(
        result
            .changes
            .iter()
            .any(|change| change.object_name().ends_with("orders"))
    );
}

#[tokio::test]
async fn failed_diff_records_completed_statement_and_marks_cache_uncertain() {
    let runtime = runtime_with_catalog_and_snapshot();
    let diff = runtime
        .diff(DiffRequest {
            left: DiffSource::SavedSnapshot(snapshot_id()),
            right: DiffSource::Live(session_id()),
            filters: DiffFilters::all(),
            renames: vec![],
        })
        .await
        .unwrap();
    let outcome = runtime.apply_diff(&diff.ordered, Some(2)).await;
    assert_eq!(outcome.completed, vec![1]);
    assert_eq!(outcome.failed, Some(2));
    assert!(outcome.catalog_state.is_uncertain());
}

fn three_batches_of(size: usize) -> Vec<Vec<Vec<dexo_driver_api::DbValue>>> {
    (0..3)
        .map(|_| {
            (0..size)
                .map(|i| vec![dexo_driver_api::DbValue::I64(i as i64)])
                .collect()
        })
        .collect()
}

#[tokio::test]
async fn export_writes_batches_without_buffering_the_dataset() {
    let sink = RecordingSink::new();
    export_row_batches(three_batches_of(1_000), sink.clone())
        .await
        .unwrap();
    assert_eq!(sink.max_rows_held(), 1_000);
    assert_eq!(sink.rows_written(), 3_000);
}

#[test]
fn file_picker_parent_hidden_and_absolute() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"ok").unwrap();
    let mut picker = FilePicker {
        cwd: dir.path().to_path_buf(),
        ..FilePicker::default()
    };
    picker.refresh();
    assert!(picker.entries.iter().any(|path| path.ends_with("a.txt")));
    let abs = picker.enter_path(dir.path().join("a.txt")).unwrap();
    assert!(abs.is_absolute());
}

fn editor_with_cursor_in_second_statement() -> (String, usize) {
    let sql = "SELECT 1;\nSELECT * FROM orders;";
    (sql.into(), sql.find("orders").unwrap())
}

#[tokio::test]
async fn explain_uses_statement_at_editor_cursor() {
    let runtime = ExplainManager::default();
    let (sql, cursor) = editor_with_cursor_in_second_statement();
    runtime.explain(&sql, cursor, false).await.unwrap();
    assert_eq!(runtime.explain_sql(), "SELECT * FROM orders");
    assert!(runtime.explain(&sql, cursor, true).await.is_err());
    runtime.confirm_analyze();
    runtime.explain(&sql, cursor, true).await.unwrap();
}

#[test]
fn create_table_change_exists() {
    let _ = SchemaChange::CreateTable {
        target: QualifiedName::new(None::<String>, Some("public"), "t"),
        def: TableDef {
            shape: TableShape::Table,
            columns: vec![ColumnSpec {
                name: QualifiedName::new(None::<String>, None::<String>, "id"),
                data_type: "int".into(),
                nullable: false,
                default_sql: None,
                identity: None,
                auto_increment: false,
                generated: None,
                primary_key: true,
            }],
            constraints: vec![],
            partition: None,
            engine: None,
            charset: None,
            collation: None,
        },
    };
}

#[test]
fn open_ddl_preview_emits_preview_effect_when_connected() {
    use dexo_tui::action::Action;
    use dexo_tui::model::Model;
    use dexo_tui::update;
    let mut model = Model {
        active_session: Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1))),
        session_generation: 1,
        ..Model::default()
    };
    let effects = update(&mut model, Action::OpenDdlPreview);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::PreviewDdl { .. }))
    );
}
