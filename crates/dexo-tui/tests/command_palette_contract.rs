use dexo_tui::palette::{FlowIntent, PaletteInvocation};

const COMMAND_IDS: [&str; 135] = [
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
    "focus.inspector",
    "layout.cycle",
    "layout.results_focus",
    "layout.hide_inspector",
    "layout.reset",
    "layout.results_grow",
    "layout.results_shrink",
    "layout.explorer_grow",
    "layout.explorer_shrink",
    "layout.inspector_grow",
    "layout.inspector_shrink",
    "data.copy.csv",
    "data.copy.text",
    "data.copy.json",
    "data.copy.markdown",
    "data.copy.sql",
    "data.apply",
    "data.revert",
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
    "admin.sessions",
    "mcp.profiles",
    "explorer.expand",
    "explorer.refresh",
    "explorer.refresh_all",
    "explorer.inspect",
    "explorer.ddl",
    "explorer.refresh_subtree",
    "explorer.up",
    "explorer.down",
    "explorer.dependencies",
    "explorer.dependents",
    "tab.sql",
    "tab.data",
    "tab.ddl",
    "tab.properties",
    "tab.explain",
    "tab.next",
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
    "inspector.next_tab",
    "settings.theme",
    "settings.keymap",
    "settings.mouse",
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
    "connection.add",
    "connection.browse",
    "connection.connect",
    "connection.duplicate",
    "connection.test",
    "connection.delete",
    "connection.close_session",
    "connection.new",
    "connection.edit",
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
    "connection.duplicate",
    "connection.test",
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
    ("connection.connect", FlowIntent::ConnectionConnect),
    ("connection.duplicate", FlowIntent::ConnectionDuplicate),
    ("connection.test", FlowIntent::ConnectionTest),
    ("connection.delete", FlowIntent::ConnectionDelete),
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
];

#[test]
fn registry_contains_each_command_exactly_once() {
    let entries = dexo_tui::palette::palette_entries(&dexo_tui::Model::default());
    let actual: std::collections::BTreeSet<_> = entries.iter().map(|e| e.id).collect();
    let expected: std::collections::BTreeSet<_> = COMMAND_IDS.into_iter().collect();
    assert_eq!(entries.len(), 135);
    assert_eq!(actual.len(), 135, "duplicate command id");
    assert_eq!(actual, expected);
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
        ("explorer.expand", "select an explorer object first"),
        ("editor.accept_completion", "no completion available"),
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
            Breadcrumb => model.data.crumbs.push((model.data.target.clone(), None, 0)),
            ActiveQuery => model.active_operation = Some(OperationId::new()),
            Completion => {
                model.set_sql("sel");
                dexo_tui::screens::editor::refresh_intelligence(model, true);
            }
            Parameters => {
                model.set_sql("select :id");
                dexo_tui::screens::editor::refresh_intelligence(model, false);
            }
            History => model.editor.history.push("select 1".into()),
            Recovery => {
                model
                    .recovery
                    .checkpoints
                    .push(("doc".into(), "now".into(), "select 1".into()))
            }
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
            Requirement::LoadedDdl => model.inspector.ddl = None,
            Requirement::PendingChanges => {
                model.data.changes = ChangeSet::for_table(&model.data.table)
            }
            Requirement::Breadcrumb => model.data.crumbs.clear(),
            Requirement::ActiveQuery => model.active_operation = None,
            Requirement::Completion => model.editor.completions.clear(),
            Requirement::Parameters => model.editor.parameters.clear(),
            Requirement::History => model.editor.history.clear(),
            Requirement::Recovery => model.recovery.checkpoints.clear(),
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

    for id in COMMAND_IDS {
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
