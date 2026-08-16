use dexo_tui::palette::{FlowIntent, PaletteInvocation};

const COMMAND_IDS: [&str; 129] = [
    "workbench.quit",
    "palette.open",
    "query.execute",
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
    "document.new",
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
    "connection.close_session",
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
    (
        "connection.close_session",
        FlowIntent::ConnectionCloseSession,
    ),
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
    assert_eq!(entries.len(), 129);
    assert_eq!(actual.len(), 129, "duplicate command id");
    assert_eq!(actual, expected);
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
