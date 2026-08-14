use dexo_tui::layout::{LayoutMode, LayoutPlan};
use dexo_tui::model::{ConnectionStatus, Model};
use dexo_tui::render::render_to_string;
use dexo_tui::screens::explorer::ExplorerState;
use ratatui::layout::Rect;

fn snapshot_model() -> Model {
    Model {
        project: "demo".into(),
        connection: ConnectionStatus {
            name: "local".into(),
            ready: true,
            environment: String::new(),
        },
        schema: "public".into(),
        sql: "select 1".into(),
        explorer: ExplorerState::fixture(),
        ..Model::default()
    }
}

#[test]
fn layout_matches_terminal() {
    for (width, height, expected) in [
        (160, 50, LayoutMode::Full),
        (100, 30, LayoutMode::Reduced),
        (60, 20, LayoutMode::Compact),
    ] {
        assert_eq!(
            LayoutPlan::for_area(Rect::new(0, 0, width, height)).mode,
            expected
        );
    }
}

#[test]
fn snapshot_160x50_full() {
    insta::assert_snapshot!(render_to_string(&snapshot_model(), 160, 50));
}

#[test]
fn snapshot_100x30_reduced() {
    insta::assert_snapshot!(render_to_string(&snapshot_model(), 100, 30));
}

#[test]
fn snapshot_explorer_offline_and_actions() {
    let mut model = snapshot_model();
    model.explorer.filter_name = "public".into();
    insta::assert_snapshot!(render_to_string(&model, 100, 30));
}

#[test]
fn snapshot_review_and_related_tab() {
    use dexo_app::data::{ColumnDef, ForeignKey, TableMeta};
    use dexo_driver_api::{DbValue, QualifiedName};
    use dexo_tui::action::Action;
    use dexo_tui::update;

    let mut model = snapshot_model();
    model.data.table = TableMeta {
        columns: vec![ColumnDef {
            name: "id".into(),
            primary_key: true,
            unique: true,
            nullable: false,
        }],
    };
    model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
    model.data.target = QualifiedName::new(Some("demo"), Some("public"), "items");
    model
        .data
        .changes
        .insert(vec![("id".into(), DbValue::I64(1))]);
    model.data.related_fk = Some(ForeignKey {
        local: vec!["id".into()],
        referenced_table: QualifiedName::new(Some("demo"), Some("public"), "users"),
        referenced: vec!["id".into()],
    });
    model.data.related_row = vec![("id".into(), Some(DbValue::I64(1)))];
    update(&mut model, Action::OpenRelated);
    update(&mut model, Action::OpenReview);
    insta::assert_snapshot!(render_to_string(&model, 100, 30));
}

#[test]
fn snapshot_schema_editor_full() {
    let mut model = snapshot_model();
    model.tabs.active = 2;
    model.schema_editor =
        dexo_tui::screens::schema_editor::SchemaEditor::table_form("public.orders");
    model.schema_editor.set_field("columns", "");
    model.schema_editor.validate();
    insta::assert_snapshot!(render_to_string(&model, 160, 50));
}

#[test]
fn snapshot_schema_editor_compact_and_preview() {
    use dexo_tui::action::Action;
    use dexo_tui::update;

    let mut model = snapshot_model();
    model.tabs.active = 2;
    model.focus = dexo_tui::model::Focus::Editor;
    model.schema_editor =
        dexo_tui::screens::schema_editor::SchemaEditor::table_form("public.orders");
    update(&mut model, Action::OpenDdlPreview);
    insta::assert_snapshot!(render_to_string(&model, 60, 20));
}

#[test]
fn snapshot_schema_diff_filters_risk_and_script() {
    use dexo_tui::action::Action;
    use dexo_tui::update;

    let mut model = snapshot_model();
    update(&mut model, Action::OpenSchemaDiff);
    insta::assert_snapshot!(render_to_string(&model, 160, 50));
    update(&mut model, Action::SchemaDiffToggleRemoved);
    insta::assert_snapshot!(render_to_string(&model, 60, 20));
}

#[test]
fn snapshot_transfer_preview_progress_rejects() {
    use dexo_tui::action::Action;
    use dexo_tui::screens::transfer::TransferScreen;
    use dexo_tui::update;

    let mut model = snapshot_model();
    update(&mut model, Action::OpenTransfer);
    insta::assert_snapshot!(render_to_string(&model, 100, 30));
    model.transfer = TransferScreen::fixture_progress();
    insta::assert_snapshot!(render_to_string(&model, 60, 20));
    model.transfer = TransferScreen::fixture_rejects();
    insta::assert_snapshot!(render_to_string(&model, 100, 30));
}

#[test]
fn snapshot_explain_tree_table_summary() {
    use dexo_tui::action::Action;
    use dexo_tui::update;

    let mut model = snapshot_model();
    update(&mut model, Action::OpenExplain);
    insta::assert_snapshot!(render_to_string(&model, 160, 50));
    update(&mut model, Action::ExplainViewTable);
    insta::assert_snapshot!(render_to_string(&model, 100, 30));
    update(&mut model, Action::ExplainViewSummary);
    insta::assert_snapshot!(render_to_string(&model, 60, 20));
}

#[test]
fn snapshot_admin_sessions_pause_and_preview() {
    use dexo_tui::action::Action;
    use dexo_tui::update;

    let mut model = snapshot_model();
    update(&mut model, Action::OpenAdmin);
    insta::assert_snapshot!(render_to_string(&model, 160, 50));
    update(&mut model, Action::AdminPause);
    insta::assert_snapshot!(render_to_string(&model, 60, 20));
}

#[test]
fn snapshot_mcp_profiles_preview_and_confirm() {
    use dexo_tui::action::Action;
    use dexo_tui::update;

    let mut model = snapshot_model();
    update(&mut model, Action::OpenMcpProfiles);
    insta::assert_snapshot!(render_to_string(&model, 160, 50));
    update(&mut model, Action::ConfirmMcpEnable);
    update(&mut model, Action::RevokeAllMcpGrants);
    insta::assert_snapshot!(render_to_string(&model, 60, 20));
}
