use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::screens::connections::ConnectionIntent;
use dexo_tui::screens::projects::{ProjectIntent, ProjectsMode};
use dexo_tui::{Action, Effect, Focus, Model, update};

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn choose_effects(model: &mut Model, query: &str) -> Vec<Effect> {
    let mut effects = update(model, Action::OpenPalette);
    for ch in query.chars() {
        effects.extend(press(model, KeyCode::Char(ch)));
    }
    effects.extend(press(model, KeyCode::Enter));
    effects
}

fn choose_effects_id(model: &mut Model, id: &str) -> Vec<Effect> {
    use dexo_tui::palette::{filter_entries, palette_entries};
    let mut effects = update(model, Action::OpenPalette);
    effects.extend(update(model, Action::PaletteQuery(id.into())));
    let entries = palette_entries(model);
    let visible = filter_entries(&entries, &model.palette.query);
    if let Some(index) = visible.iter().position(|entry| entry.id == id) {
        model.palette.selected = index;
    }
    effects.extend(press(model, KeyCode::Enter));
    effects
}

fn choose(model: &mut Model, query: &str) {
    let _ = choose_effects(model, query);
}

#[test]
fn escape_restores_palette_origin_focus() {
    let mut model = Model {
        focus: Focus::Results,
        ..Model::default()
    };
    update(&mut model, Action::OpenPalette);
    assert_eq!(model.focus, Focus::Palette);
    press(&mut model, KeyCode::Esc);
    assert_eq!(model.focus, Focus::Results);
}

#[test]
fn project_create_opens_the_existing_name_form() {
    let mut model = Model::default();
    choose(&mut model, "project.create");
    assert!(!model.palette.open);
    assert!(model.projects.open);
    assert_eq!(model.projects.mode, ProjectsMode::Create);
    assert!(model.projects.name_input.is_empty());
}

#[test]
fn palette_renders_registered_shortcut() {
    let mut model = Model::default();
    update(&mut model, Action::OpenPalette);
    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(view.contains("Ctrl+P"));
}

#[test]
fn project_create_preserves_invalid_input() {
    let mut model = Model::default();
    choose(&mut model, "project.create");
    press(&mut model, KeyCode::Enter);
    assert!(model.projects.open);
    assert_eq!(model.projects.mode, ProjectsMode::Create);
    assert_eq!(
        model.projects.error.as_deref(),
        Some("project name is required")
    );
}

#[test]
fn project_rename_loads_a_visible_chooser_before_input() {
    let mut model = Model::default();
    let effects = choose_effects(&mut model, "project.rename");
    assert!(model.projects.open);
    assert_eq!(model.projects.intent, Some(ProjectIntent::Rename));
    assert!(matches!(effects.as_slice(), [Effect::ListProjects]));
}

#[test]
fn connection_delete_opens_browser_and_never_hides_confirmation() {
    let mut model = Model::default();
    choose(&mut model, "connection.delete");
    assert!(model.connections.open);
    assert_eq!(model.connections.intent, Some(ConnectionIntent::Delete));
    assert!(model.connections.delete_target.is_none());
}

fn active_transaction_model() -> Model {
    use dexo_driver_api::TransactionState;
    Model {
        focus: Focus::Editor,
        transaction: TransactionState::Active,
        active_session: Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1))),
        ..Model::default()
    }
}

#[test]
fn savepoint_asks_for_a_name_instead_of_using_sp1() {
    let mut model = active_transaction_model();
    choose(&mut model, "transaction.savepoint");
    assert!(model.transaction_prompt.open);
    assert!(model.transaction_prompt.name.is_empty());
}

enum ObservedOutcome {
    Effects,
    VisibleFlow,
    VisibleDisabledReason,
    DirectStateChange,
}

fn model_satisfying(requirements: &[dexo_tui::palette::Requirement]) -> Model {
    use dexo_app::data::{ChangeSet, ColumnDef, TableMeta};
    use dexo_driver_api::{DbValue, ObjectId};
    use dexo_tui::palette::Requirement;
    use dexo_tui::runtime::{OperationId, SessionId};
    let mut model = Model::default();
    for requirement in requirements {
        match requirement {
            Requirement::ActiveSession => {
                model.active_session = Some(SessionId(uuid::Uuid::from_u128(1)));
                model.session_generation = 1;
            }
            Requirement::Results => {
                model.results.append_rows(vec![
                    vec![DbValue::I64(1), DbValue::I64(2), DbValue::I64(3)],
                    vec![DbValue::I64(4), DbValue::I64(5), DbValue::I64(6)],
                    vec![DbValue::I64(7), DbValue::I64(8), DbValue::I64(9)],
                ]);
                model.results.select_cell(1, 1);
                model.results.scroll_columns(1);
                model.results.tabs.push(dexo_tui::ResultTab::new(
                    dexo_tui::ResultKey {
                        operation: dexo_tui::runtime::OperationKey::new(
                            dexo_tui::runtime::OperationId::new(),
                            "",
                            "",
                            0,
                        ),
                        index: 1,
                    },
                    "result-2",
                ));
            }
            Requirement::RowSelection => {
                if model.results.rows().is_empty() {
                    model.results.append_rows(vec![
                        vec![DbValue::I64(1), DbValue::I64(2), DbValue::I64(3)],
                        vec![DbValue::I64(4), DbValue::I64(5), DbValue::I64(6)],
                        vec![DbValue::I64(7), DbValue::I64(8), DbValue::I64(9)],
                    ]);
                }
                model.results.select_cell(1, 1);
            }
            Requirement::ExplorerNode => {
                let selected = ObjectId::new("table:items");
                model.explorer.roots = ["users", "items", "orders"]
                    .into_iter()
                    .map(|name| dexo_tui::screens::explorer::ExplorerNode {
                        id: ObjectId::new(format!("table:{name}")),
                        label: name.into(),
                        kind: dexo_driver_api::ObjectKind::Table,
                        qualified: format!("public.{name}"),
                        schema: Some("public".into()),
                        state: dexo_tui::screens::explorer::NodeState::Collapsed,
                        expanded: false,
                        favorite: false,
                        children: Vec::new(),
                        restriction: None,
                        error: None,
                    })
                    .collect();
                model.explorer.selected = Some(selected);
            }
            Requirement::LoadedDdl => {
                model.inspector.ddl = Some("create table items(id bigint)".into())
            }
            Requirement::PendingChanges => {
                model.data.table = TableMeta {
                    columns: vec![ColumnDef {
                        name: "id".into(),
                        primary_key: true,
                        unique: true,
                        nullable: false,
                    }],
                };
                model.data.changes = ChangeSet::for_table(&model.data.table);
                model
                    .data
                    .changes
                    .insert(vec![("id".into(), DbValue::I64(1))]);
            }
            Requirement::Breadcrumb => model.data.crumbs.push((model.data.target.clone(), None, 0)),
            Requirement::ActiveQuery => model.active_operation = Some(OperationId::new()),
            Requirement::Completion => {
                model.set_sql("sel");
                dexo_tui::screens::editor::refresh_intelligence(&mut model, true);
            }
            Requirement::Parameters => {
                model.set_sql("select :id");
                dexo_tui::screens::editor::refresh_intelligence(&mut model, false);
            }
            Requirement::History => model.editor.history.push("select 1".into()),
            Requirement::Recovery => {
                model
                    .recovery
                    .checkpoints
                    .push(("doc".into(), "now".into(), "select 1".into()))
            }
        }
    }
    if model.active_document().text().is_empty() {
        model.set_sql("select 1");
    }
    model
}

fn apply_transaction_context(id: &str, model: &mut Model) {
    use dexo_driver_api::TransactionState;
    match id {
        "transaction.savepoint"
        | "transaction.release_savepoint"
        | "transaction.commit"
        | "transaction.rollback_savepoint"
        | "transaction.rollback" => {
            model.transaction = TransactionState::Active;
        }
        _ => {}
    }
}

fn observe_command(id: &str) -> ObservedOutcome {
    use dexo_tui::palette::{PaletteInvocation, command_spec};
    let spec = command_spec(id).unwrap();
    let mut model = model_satisfying(spec.requirements);
    apply_transaction_context(id, &mut model);
    model.tabs.scroll = 1;
    if model.tabs.active == 0 && id == "tab.sql" {
        model.tabs.active = 1;
    }
    if model.documents.len() < 2 {
        model
            .documents
            .push(dexo_tui::model::EditorDocument::with_text("select 2"));
    }
    let before = model.clone();
    let before_view = dexo_tui::render::render_to_string(&before, 100, 30);
    let effects = choose_effects_id(&mut model, id);
    let after_view = dexo_tui::render::render_to_string(&model, 100, 30);

    if !effects.is_empty() {
        return ObservedOutcome::Effects;
    }
    if model.palette.open && !model.messages.is_empty() {
        return ObservedOutcome::VisibleDisabledReason;
    }
    if matches!(spec.invocation, PaletteInvocation::OpenFlow(_)) && before_view != after_view {
        return ObservedOutcome::VisibleFlow;
    }
    if model.palette.open != before.palette.open {
        return ObservedOutcome::DirectStateChange;
    }

    let mut normalized = model;
    normalized.palette = before.palette.clone();
    normalized.focus = before.focus;
    assert_ne!(
        normalized, before,
        "{id} closed the palette as a silent no-op"
    );
    ObservedOutcome::DirectStateChange
}

#[test]
fn every_palette_id_has_an_observable_outcome() {
    use dexo_tui::palette::{PaletteInvocation, command_spec, palette_entries};
    for entry in palette_entries(&Model::default()) {
        let id = entry.id;
        let spec = command_spec(id).unwrap();
        let observed = observe_command(id);
        match spec.invocation {
            PaletteInvocation::OpenFlow(_) => assert!(
                matches!(
                    observed,
                    ObservedOutcome::Effects | ObservedOutcome::VisibleFlow
                ),
                "{id} did not open or start its declared flow",
            ),
            PaletteInvocation::Dispatch(_) => assert!(
                !matches!(observed, ObservedOutcome::VisibleDisabledReason),
                "ready command {id} remained disabled"
            ),
        }
    }
}
