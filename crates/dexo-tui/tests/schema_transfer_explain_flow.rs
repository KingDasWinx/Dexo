use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dexo_app::schema::{CatalogScope, Confirmation, ConfirmationAnswer};
use dexo_app::schema_diff::{DiffSource, SchemaSnapshot};
use dexo_app::transfer::{RecordingSink, export_row_batches};
use dexo_driver_api::{
    AlterOp, CatalogObject, ColumnSpec, DdlExecutor, DdlOutcome, DdlPlan, DriverError, ObjectId,
    ObjectKind, QualifiedName, SchemaChange, TableDef, TableShape,
};
use dexo_tui::runtime::explain_manager::statement_sql;
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
    assert!(
        picker
            .entries
            .iter()
            .any(|entry| entry.path.ends_with("a.txt"))
    );
    let abs = picker.enter_path(dir.path().join("a.txt")).unwrap();
    assert!(abs.is_absolute());
}

fn editor_with_cursor_in_second_statement() -> (String, usize) {
    let sql = "SELECT 1;\nSELECT * FROM orders;";
    (sql.into(), sql.find("orders").unwrap())
}

#[tokio::test]
async fn explain_uses_statement_at_editor_cursor() {
    let (sql, cursor) = editor_with_cursor_in_second_statement();
    assert_eq!(
        statement_sql(&sql, cursor, dexo_sql::Dialect::Postgres).as_deref(),
        Some("SELECT * FROM orders")
    );
    // Split as the connection's dialect reads it: a SQLite `[a;b]` is one name, and a
    // MySQL `#` comment's apostrophe opens no string.
    assert_eq!(
        statement_sql(
            "select [a;b] from t; select 2",
            3,
            dexo_sql::Dialect::Sqlite
        )
        .as_deref(),
        Some("select [a;b] from t")
    );
    let mysql = "select 1; # it's\nselect 2;\nselect 3";
    assert_eq!(
        statement_sql(
            mysql,
            mysql.find("select 2").unwrap(),
            dexo_sql::Dialect::Mysql
        )
        .as_deref(),
        Some("select 2")
    );
}

fn transfer_session() -> dexo_tui::runtime::SessionId {
    dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1))
}

fn transfer_ready_model() -> dexo_tui::Model {
    let mut model = dexo_tui::Model {
        active_session: Some(transfer_session()),
        ..dexo_tui::Model::default()
    };
    model
        .results
        .append_rows(vec![vec![dexo_driver_api::DbValue::I64(1)]]);
    model
}

fn choose_effects(model: &mut dexo_tui::Model, query: &str) -> Vec<dexo_tui::Effect> {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use dexo_tui::{Action, update};
    let mut effects = update(model, Action::OpenPalette);
    for ch in query.chars() {
        effects.extend(update(
            model,
            Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
        ));
    }
    effects.extend(update(
        model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    ));
    effects
}

fn choose(model: &mut dexo_tui::Model, query: &str) {
    let _ = choose_effects(model, query);
}

fn recording_transfer_runtime() -> dexo_tui::runtime::transfer_manager::TransferManager {
    dexo_tui::runtime::transfer_manager::TransferManager::default()
}

#[test]
fn every_transfer_palette_command_opens_its_own_mode() {
    use dexo_tui::screens::transfer::TransferMode;
    for (id, expected) in [
        ("transfer.export", TransferMode::Export),
        ("transfer.import", TransferMode::Import),
        ("backup.dump", TransferMode::Backup),
        ("backup.restore", TransferMode::Restore),
    ] {
        let mut model = transfer_ready_model();
        choose(&mut model, id);
        assert_eq!(model.transfer.mode, expected);
    }
}

#[tokio::test]
async fn import_and_restore_never_write_to_the_source_path() {
    use dexo_tui::action::TransferRequest;
    use dexo_tui::screens::transfer::TransferMode;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.dump");
    std::fs::write(&source, b"ORIGINAL").unwrap();
    let runtime = recording_transfer_runtime();
    runtime
        .run(TransferRequest::restore(source.clone(), transfer_session()))
        .await
        .unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), b"ORIGINAL");
    assert_eq!(runtime.recorded_modes(), vec![TransferMode::Restore]);
}

fn connected_model() -> dexo_tui::Model {
    transfer_ready_model()
}

fn press(model: &mut dexo_tui::Model, code: crossterm::event::KeyCode) -> Vec<dexo_tui::Effect> {
    dexo_tui::update(
        model,
        dexo_tui::Action::Key(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        )),
    )
}

#[test]
fn schema_diff_command_starts_loading_instead_of_opening_empty_default() {
    let mut model = connected_model();
    choose(&mut model, "schema.diff");
    assert_eq!(model.screen, dexo_tui::model::Screen::Compare);
    assert!(model.schema_diff.source_prompt);
    assert!(model.schema_diff.entries.is_empty());
}

/// Manage Grants is the Privileges view of a table's document: with none in front,
/// nothing is read and the person is told to open one.
#[test]
fn manage_grants_is_the_privileges_view_of_a_table() {
    let mut model = connected_model();
    let effects = choose_effects(&mut model, "schema.security");
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::LoadSecurity { .. })),
        "{effects:?}"
    );
    model.active_document_mut().kind =
        dexo_tui::model::DocumentKind::Table(dexo_app::parse_qualified("local.public.orders"));
    let effects = choose_effects(&mut model, "schema.security");
    assert_eq!(model.results.view, dexo_tui::model::ResultsView::Privileges);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::LoadSecurity { .. })),
        "{effects:?}"
    );
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

fn connected_model_with_sql(sql: &str) -> dexo_tui::Model {
    let mut model = connected_model();
    model.set_sql(sql);
    model
}

#[test]
fn explain_effect_carries_second_statement_cursor() {
    use dexo_tui::action::{Action, Effect};
    use dexo_tui::update;
    let mut model = connected_model_with_sql("select 1;\nselect 2;");
    model
        .active_document_mut()
        .sql
        .set_cursor("select 1;\nselect ".chars().count())
        .unwrap();
    let effects = update(&mut model, Action::OpenExplain);
    assert!(matches!(
        effects.as_slice(),
        [Effect::RunExplain { cursor, .. }] if *cursor > 0
    ));
}

/// Import and Restore write into the database, through the driver and through native
/// tools whose connections never get the read-only setting; a read-only connection
/// refuses them before anything starts.
#[test]
fn a_read_only_connection_refuses_import_and_restore() {
    use dexo_tui::screens::transfer::TransferMode;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("rows.csv");
    std::fs::write(&source, b"id\n1\n").unwrap();
    let started = |read_only: bool, mode: TransferMode| {
        let mut model = transfer_ready_model();
        model.connection.read_only = read_only;
        choose(
            &mut model,
            match mode {
                TransferMode::Import => "transfer.import",
                _ => "backup.restore",
            },
        );
        model.transfer.path.set_text(source.display().to_string());
        model.transfer.table.set_text("rows");
        // A restore asks once more before it writes into the database.
        let mut effects = press(&mut model, crossterm::event::KeyCode::Enter);
        if mode == TransferMode::Restore && model.transfer.confirm.is_some() {
            model.transfer.footer = dexo_tui::widgets::form::FooterFocus::Submit;
            effects = press(&mut model, crossterm::event::KeyCode::Enter);
        }
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::RunTransfer(_)))
    };
    for mode in [TransferMode::Import, TransferMode::Restore] {
        assert!(
            started(false, mode),
            "{mode:?} does not start even when writable"
        );
        assert!(
            !started(true, mode),
            "{mode:?} started on a read-only connection"
        );
    }
}

/// An SQL export writes the connection's dialect: a SQLite blob as `X'..'`, which
/// SQLite reads back as a blob, not Postgres's `'\x..'`, which it reads as text.
#[tokio::test]
async fn an_sql_export_speaks_the_connections_dialect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rows.sql");
    let manager = dexo_tui::runtime::transfer_manager::TransferManager::default();
    manager
        .run_with(
            dexo_tui::action::TransferRequest::Export {
                operation: dexo_tui::runtime::OperationId::new(),
                path: path.clone(),
                format: dexo_app::transfer::TransferFormat::Sql,
                columns: vec!["data".into()],
                rows: std::sync::Arc::new(vec![vec![dexo_driver_api::DbValue::Bytes(vec![
                    0xca, 0xfe,
                ])]]),
                dialect: dexo_app::data::SqlDialect::Sqlite,
                table: Some("blobs".into()),
            },
            None,
        )
        .await
        .unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("X'cafe'"), "{written}");
    // The table the request names, not one called after the file.
    assert!(written.contains("INSERT INTO \"blobs\""), "{written}");
}

/// Ctrl+F cycles the formats; an import never lands on SQL, which is a script to run,
/// not data it can read, while an export does.
#[test]
fn an_import_never_offers_sql() {
    let formats = |id: &str| {
        let mut model = transfer_ready_model();
        choose(&mut model, id);
        (0..6)
            .map(|_| {
                dexo_tui::update(
                    &mut model,
                    dexo_tui::Action::Key(crossterm::event::KeyEvent::new(
                        crossterm::event::KeyCode::Char('f'),
                        crossterm::event::KeyModifiers::CONTROL,
                    )),
                );
                model.transfer.format.clone()
            })
            .collect::<Vec<_>>()
    };
    assert!(!formats("transfer.import").contains(&"sql".to_string()));
    assert!(formats("transfer.export").contains(&"sql".to_string()));
}
