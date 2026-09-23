use dexo_tui::palette::{FlowIntent, PaletteInvocation};

const COMMAND_IDS: &[&str] = &[
    "workbench.quit",
    "palette.open",
    "query.execute_statement",
    "query.execute_selection",
    "query.execute_document",
    "query.cancel",
    "transaction.begin",
    "transaction.savepoint",
    "transaction.rollback_savepoint",
    "transaction.release_savepoint",
    "transaction.commit",
    "transaction.rollback",
    "help.open",
    "focus.explorer",
    "focus.editor",
    "focus.results",
    "focus.tabs",
    "document.activate_tab",
    "document.tab_prev",
    "document.tab_next",
    "layout.cycle",
    "layout.hide_explorer",
    "layout.hide_results",
    "layout.reset",
    "layout.results_grow",
    "layout.results_shrink",
    "layout.explorer_grow",
    "layout.explorer_shrink",
    "data.copy.csv",
    "data.copy.text",
    "data.copy.json",
    "data.copy.markdown",
    "data.copy.sql",
    "data.apply",
    "data.revert",
    "data.discard_all",
    "data.refresh",
    "data.toggle_delete",
    "data.insert_row",
    "data.nav_back",
    "data.page_next",
    "data.page_prev",
    "data.sort",
    "data.filter",
    "data.review",
    "data.related",
    "data.inspect",
    "schema.preview",
    "schema.raw",
    "schema.diff",
    "transfer.export",
    "transfer.import",
    "backup.dump",
    "backup.restore",
    "schema.security",
    "explain.open",
    "results.cycle_view",
    "explain.analyze",
    "admin.sessions",
    "mcp.profiles",
    "explorer.expand",
    "explorer.refresh",
    "explorer.refresh_all",
    "explorer.inspect",
    "explorer.ddl",
    "explorer.up",
    "explorer.down",
    "explorer.dependencies",
    "document.next",
    "document.prev",
    "document.next_focus",
    "document.prev_focus",
    "document.close",
    "document.new",
    "document.rename",
    "document.save",
    "document.open",
    "results.select_row",
    "results.select_column",
    "results.next_tab",
    "results.prev_tab",
    "settings.mode",
    "settings.accent",
    "settings.keymap",
    "settings.mouse",
    "settings.animation",
    "settings.unicode",
    "explorer.data",
    "editor.goto",
    "explorer.copy_name",
    "explorer.copy_simple",
    "explorer.copy_ddl",
    "explorer.favorite",
    "explorer.favorites_only",
    "explorer.system_objects",
    "results.up",
    "results.down",
    "results.left",
    "results.right",
    "results.pageup",
    "results.pagedown",
    "results.top",
    "results.extend_up",
    "results.extend_down",
    "results.actions",
    "results.toggle_pick",
    "connection.browse",
    "connection.close_session",
    "connection.new",
    "connection.edit",
    "connection.test",
    "connection.duplicate",
    "connection.move_group",
    "connection.delete",
    "explorer.actions",
    "project.browse",
    "project.switch",
    "project.create",
    "project.rename",
    "project.delete",
    "config.transfer",
    "settings.open",
    "settings.reset",
    "recovery.open",
    "recovery.restore",
    "recovery.discard",
    "mcp.audit",
    "mcp.revoke_all",
    "editor.complete",
    "editor.format",
    "editor.undo",
    "editor.redo",
    "editor.select_all",
    "editor.paste",
    "editor.accept_completion",
    "editor.snippet",
    "editor.parameters",
    "editor.history",
    "editor.history.clear",
    "diagnostics.export",
];

const FLOW_IDS: &[&str] = &[
    "transaction.savepoint",
    "transaction.rollback_savepoint",
    "transaction.release_savepoint",
    "data.sort",
    "data.filter",
    "data.review",
    "schema.preview",
    "schema.raw",
    "schema.diff",
    "schema.security",
    "transfer.export",
    "transfer.import",
    "backup.dump",
    "backup.restore",
    "connection.connect",
    "connection.delete",
    "project.switch",
    "project.create",
    "project.rename",
    "project.delete",
    "settings.reset",
    "recovery.restore",
    "recovery.discard",
    "mcp.revoke_all",
    "editor.snippet",
    "editor.parameters",
    "editor.history.clear",
    "diagnostics.export",
];

const FLOW_INTENTS: &[(&str, FlowIntent)] = &[
    ("transaction.savepoint", FlowIntent::SavepointCreate),
    (
        "transaction.rollback_savepoint",
        FlowIntent::SavepointRollback,
    ),
    (
        "transaction.release_savepoint",
        FlowIntent::SavepointRelease,
    ),
    ("data.sort", FlowIntent::DataSort),
    ("data.filter", FlowIntent::DataFilter),
    ("data.review", FlowIntent::DataReview),
    ("schema.preview", FlowIntent::SchemaPreview),
    ("schema.raw", FlowIntent::SchemaRaw),
    ("schema.diff", FlowIntent::SchemaDiff),
    ("schema.security", FlowIntent::Security),
    ("transfer.export", FlowIntent::TransferExport),
    ("transfer.import", FlowIntent::TransferImport),
    ("backup.dump", FlowIntent::Backup),
    ("backup.restore", FlowIntent::Restore),
    ("project.switch", FlowIntent::ProjectSwitch),
    ("project.create", FlowIntent::ProjectCreate),
    ("project.rename", FlowIntent::ProjectRename),
    ("project.delete", FlowIntent::ProjectDelete),
    ("settings.reset", FlowIntent::SettingsReset),
    ("recovery.restore", FlowIntent::RecoveryRestore),
    ("recovery.discard", FlowIntent::RecoveryDiscard),
    ("mcp.revoke_all", FlowIntent::McpRevokeAll),
    ("editor.snippet", FlowIntent::InsertSnippet),
    ("editor.parameters", FlowIntent::SubmitParameters),
    ("editor.history.clear", FlowIntent::ClearHistory),
    ("diagnostics.export", FlowIntent::DiagnosticsExport),
    ("connection.delete", FlowIntent::ConnectionDelete),
];

#[test]
fn registry_contains_each_command_exactly_once() {
    let specs = dexo_tui::palette::command_specs();
    let actual: std::collections::BTreeSet<_> = specs.iter().map(|s| s.id).collect();
    let expected: std::collections::BTreeSet<_> = COMMAND_IDS.iter().copied().collect();
    assert_eq!(specs.len(), 140);
    assert_eq!(actual.len(), 140, "duplicate command id");
    assert_eq!(actual, expected);
}

/// The curated subset. Without pinning it the palette silently re-inflates, one
/// well-meaning `CommandSpec` at a time.
#[test]
fn palette_shows_only_the_curated_subset() {
    let visible = dexo_tui::palette::palette_entries(&dexo_tui::Model::default());
    assert_eq!(visible.len(), 88);
}

/// A category with no display name falls back to the raw prefix, which looks like a
/// bug in the gutter. Fail loudly the day someone adds `bookmark.save`.
/// The help overlay used to resolve titles through the *visible* palette, so every
/// demoted-but-bound command would have rendered as a raw dotted id.
#[test]
fn help_shows_titles_not_command_ids() {
    let mut model = dexo_tui::Model::default();
    dexo_tui::update(&mut model, dexo_tui::action::Action::ToggleHelp);
    let view = dexo_tui::render::render_to_string(&model, 120, 40);
    for spec in dexo_tui::palette::command_specs() {
        assert!(
            !view.contains(spec.id),
            "help rendered the raw id `{}` instead of its title",
            spec.id
        );
    }
}

#[test]
fn every_command_prefix_has_a_category_label() {
    for spec in dexo_tui::palette::command_specs() {
        let prefix = spec.id.split('.').next().unwrap();
        let label = dexo_tui::palette::category_label(spec.id);
        assert_ne!(
            label, prefix,
            "{} has no entry in CATEGORIES (fell back to the raw prefix)",
            spec.id
        );
    }
}

/// Demoting a command must not amount to deleting it: every id a keymap binds has to
/// stay resolvable, palette or not.
#[test]
fn demoted_commands_stay_reachable_by_key() {
    use dexo_tui::keymap::Keymap;

    let model = dexo_tui::Model::default();
    let visible: std::collections::BTreeSet<_> = dexo_tui::palette::palette_entries(&model)
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    let mut demoted_and_bound = 0;
    for keymap in [
        Keymap::default_profile(),
        Keymap::vim_profile(),
        Keymap::emacs_profile(),
    ] {
        for id in keymap.command_ids() {
            assert!(
                dexo_tui::palette::invocation_by_id(&model, id).is_some(),
                "bound command {id} is unreachable"
            );
            if !visible.contains(id) {
                demoted_and_bound += 1;
            }
        }
    }
    assert!(
        demoted_and_bound > 0,
        "no demoted command is keybound -- the test is not exercising the split"
    );
}

#[test]
fn query_commands_expose_one_action_per_execution_scope() {
    let entries = dexo_tui::palette::palette_entries(&dexo_tui::Model::default());
    let actual: Vec<_> = entries
        .iter()
        .filter(|entry| entry.id.starts_with("query.execute"))
        .map(|entry| (entry.id, entry.shortcut))
        .collect();

    assert_eq!(
        actual,
        vec![
            ("query.execute_statement", Some("Ctrl+Enter")),
            ("query.execute_selection", None),
            ("query.execute_document", Some("Ctrl+Shift+F10")),
        ]
    );
}

#[test]
fn every_command_declares_direct_or_flow_invocation() {
    let entries = dexo_tui::palette::palette_entries(&dexo_tui::Model::default());
    for entry in &entries {
        match &entry.invocation {
            PaletteInvocation::OpenFlow(intent) => {
                assert!(FLOW_IDS.contains(&entry.id), "unexpected flow {}", entry.id);
                let expected = FLOW_INTENTS
                    .iter()
                    .find(|(id, _)| *id == entry.id)
                    .map(|(_, intent)| intent);
                assert_eq!(expected, Some(intent), "{}", entry.id);
            }
            PaletteInvocation::Dispatch(_) => {
                assert!(
                    !FLOW_IDS.contains(&entry.id),
                    "{} must open a flow",
                    entry.id
                );
            }
        }
    }
}

#[test]
fn default_model_explains_missing_context() {
    let entries = dexo_tui::palette::palette_entries(&dexo_tui::Model::default());
    for (id, reason) in [
        ("query.execute_statement", "connect a session first"),
        ("data.copy.csv", "no results available"),
        ("explorer.copy_name", "select an explorer object first"),
        ("editor.history.clear", "history is empty"),
    ] {
        let entry = entries.iter().find(|entry| entry.id == id).unwrap();
        assert_eq!(entry.disabled_reason.as_deref(), Some(reason));
    }
}

#[test]
fn every_context_command_has_a_reason_then_becomes_actionable() {
    use dexo_app::data::{ChangeSet, ColumnDef, TableMeta};
    use dexo_driver_api::{DbValue, ObjectId, TransactionState};
    use dexo_tui::Model;
    use dexo_tui::palette::{Requirement, command_spec, palette_entries};
    use dexo_tui::runtime::{OperationId, SessionId};

    fn satisfy(model: &mut Model, requirement: Requirement) {
        use Requirement::*;
        match requirement {
            ActiveSession => {
                model.active_session = Some(SessionId(uuid::Uuid::from_u128(1)));
                model.session_generation = 1;
                model.connection.name = "local".into();
            }
            Results => {
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
            RowSelection => {
                if model.results.rows().is_empty() {
                    model.results.append_rows(vec![
                        vec![DbValue::I64(1), DbValue::I64(2), DbValue::I64(3)],
                        vec![DbValue::I64(4), DbValue::I64(5), DbValue::I64(6)],
                        vec![DbValue::I64(7), DbValue::I64(8), DbValue::I64(9)],
                    ]);
                }
                model.results.select_cell(1, 1);
            }
            SelectedConnection => {
                model.connections.load_profiles(vec![dexo_app::ConnectionProfile::new(
                    dexo_app::ConnectionId(uuid::Uuid::nil()),
                    None,
                    "prod",
                    "postgres",
                    "local",
                    serde_json::json!({"host":"localhost","port":5432,"username":"u","database":"d"}),
                    dexo_app::SecretRef::new("ref-1".into()),
                )]);
            }
            ExplorerNode => {
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
                        type_name: None,
                        children: Vec::new(),
                        restriction: None,
                        error: None,
                    })
                    .collect();
                model.explorer.selected = Some(selected);
            }
            LoadedDdl => model.inspector.ddl = Some("create table items(id bigint)".into()),
            PendingChanges => {
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
            ActiveQuery => model.active_operation = Some(OperationId::new()),
            Parameters => {
                model.set_sql("select :id");
                dexo_tui::screens::editor::refresh_intelligence(model, false);
            }
            History => model.editor.history.push("select 1".into()),
        }
    }

    fn model_satisfying(requirements: &[Requirement]) -> Model {
        let mut model = Model::default();
        for requirement in requirements {
            satisfy(&mut model, *requirement);
        }
        model
    }

    fn model_missing(requirements: &[Requirement], missing: Requirement) -> Model {
        let mut model = model_satisfying(requirements);
        match missing {
            Requirement::ActiveSession => model.active_session = None,
            Requirement::Results => model.results.clear(),
            Requirement::RowSelection => model.results.select_column(0),
            Requirement::ExplorerNode => model.explorer.selected = None,
            Requirement::SelectedConnection => model.connections.load_profiles(Vec::new()),
            Requirement::LoadedDdl => model.inspector.ddl = None,
            Requirement::PendingChanges => {
                model.data.changes = ChangeSet::for_table(&model.data.table)
            }
            Requirement::ActiveQuery => model.active_operation = None,
            Requirement::Parameters => model.editor.parameters.clear(),
            Requirement::History => model.editor.history.clear(),
        }
        model
    }

    fn apply_transaction_context(id: &str, model: &mut Model) {
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

    for id in palette_entries(&Model::default())
        .into_iter()
        .map(|entry| entry.id)
    {
        let requirements = command_spec(id).unwrap().requirements;
        let mut ready_model = model_satisfying(requirements);
        apply_transaction_context(id, &mut ready_model);
        let ready = palette_entries(&ready_model)
            .into_iter()
            .find(|entry| entry.id == id)
            .unwrap();
        assert!(ready.disabled_reason.is_none(), "{id}");

        for requirement in requirements {
            let blocked_model = model_missing(requirements, *requirement);
            let blocked = palette_entries(&blocked_model)
                .into_iter()
                .find(|entry| entry.id == id)
                .unwrap();
            assert_eq!(
                blocked.disabled_reason.as_deref(),
                Some(requirement.reason()),
                "{id} did not explain {requirement:?}",
            );
        }
    }
}
