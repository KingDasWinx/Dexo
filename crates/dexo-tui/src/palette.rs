use crate::action::{Action, FocusTarget};
use crate::model::Model;
use dexo_driver_api::TransactionState;

#[derive(Clone, Debug)]
pub struct PaletteEntry {
    pub id: &'static str,
    pub title: &'static str,
    pub keywords: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub disabled_reason: Option<&'static str>,
    pub action: fn() -> Action,
}

pub fn palette_entries(model: &Model) -> Vec<PaletteEntry> {
    vec![
        PaletteEntry {
            id: "workbench.quit",
            title: "Quit",
            keywords: &["exit", "close"],
            shortcut: Some("Ctrl+Q"),
            disabled_reason: None,
            action: || Action::Quit,
        },
        PaletteEntry {
            id: "palette.open",
            title: "Command Palette",
            keywords: &["commands", "search"],
            shortcut: Some("Ctrl+P"),
            disabled_reason: None,
            action: || Action::OpenPalette,
        },
        PaletteEntry {
            id: "query.execute",
            title: "Execute Query",
            keywords: &["run", "sql"],
            shortcut: Some("F5"),
            disabled_reason: if model.active_document().text().trim().is_empty() {
                Some("editor is empty")
            } else {
                None
            },
            action: || Action::ExecuteQuery,
        },
        PaletteEntry {
            id: "query.execute_statement",
            title: "Execute Statement",
            keywords: &["run", "sql", "cursor"],
            shortcut: Some("Ctrl+Enter"),
            disabled_reason: if model.active_document().text().trim().is_empty() {
                Some("editor is empty")
            } else {
                None
            },
            action: || Action::ExecuteStatement,
        },
        PaletteEntry {
            id: "query.execute_selection",
            title: "Execute Selection",
            keywords: &["run", "sql", "selected"],
            shortcut: None,
            disabled_reason: if model.active_document().selection().is_none() {
                Some("no selection")
            } else {
                None
            },
            action: || Action::ExecuteSelection,
        },
        PaletteEntry {
            id: "query.execute_document",
            title: "Execute Document",
            keywords: &["run", "sql", "all"],
            shortcut: Some("F5"),
            disabled_reason: if model.active_document().text().trim().is_empty() {
                Some("editor is empty")
            } else {
                None
            },
            action: || Action::ExecuteDocument,
        },
        PaletteEntry {
            id: "query.cancel",
            title: "Cancel Query",
            keywords: &["stop", "abort"],
            shortcut: Some("Ctrl+C"),
            disabled_reason: if model.active_query.is_none() {
                Some("no running query")
            } else {
                None
            },
            action: || Action::CancelQuery,
        },
        PaletteEntry {
            id: "transaction.begin",
            title: "Begin Transaction",
            keywords: &["tx", "begin", "start"],
            shortcut: None,
            disabled_reason: if model.active_session.is_some()
                && model.transaction == TransactionState::Idle
            {
                None
            } else {
                Some("session is not idle")
            },
            action: || Action::BeginTransaction,
        },
        PaletteEntry {
            id: "transaction.savepoint",
            title: "Create Savepoint",
            keywords: &["tx", "savepoint"],
            shortcut: None,
            disabled_reason: if model.transaction == TransactionState::Active {
                None
            } else {
                Some("no active transaction")
            },
            action: || Action::Savepoint,
        },
        PaletteEntry {
            id: "transaction.rollback_savepoint",
            title: "Rollback Savepoint",
            keywords: &["tx", "savepoint", "rollback"],
            shortcut: None,
            disabled_reason: if model.transaction == TransactionState::Active {
                None
            } else {
                Some("no active transaction")
            },
            action: || Action::RollbackSavepoint,
        },
        PaletteEntry {
            id: "transaction.release_savepoint",
            title: "Release Savepoint",
            keywords: &["tx", "savepoint", "release"],
            shortcut: None,
            disabled_reason: if model.transaction == TransactionState::Active {
                None
            } else {
                Some("no active transaction")
            },
            action: || Action::ReleaseSavepoint,
        },
        PaletteEntry {
            id: "transaction.commit",
            title: "Commit Transaction",
            keywords: &["tx", "commit"],
            shortcut: None,
            disabled_reason: if model.transaction == TransactionState::Active {
                None
            } else {
                Some("no active transaction")
            },
            action: || Action::CommitTransaction,
        },
        PaletteEntry {
            id: "transaction.rollback",
            title: "Rollback Transaction",
            keywords: &["tx", "abort"],
            shortcut: None,
            disabled_reason: if matches!(
                model.transaction,
                TransactionState::Active | TransactionState::Failed
            ) {
                None
            } else {
                Some("no active transaction")
            },
            action: || Action::RollbackTransaction,
        },
        PaletteEntry {
            id: "help.open",
            title: "Show Keybindings",
            keywords: &["help", "keys", "cheatsheet", "shortcuts"],
            shortcut: Some("F1"),
            disabled_reason: None,
            action: || Action::ToggleHelp,
        },
        PaletteEntry {
            id: "focus.explorer",
            title: "Focus Explorer",
            keywords: &["sidebar", "tree"],
            shortcut: Some("Alt+1"),
            disabled_reason: None,
            action: || Action::Focus(FocusTarget::Explorer),
        },
        PaletteEntry {
            id: "focus.editor",
            title: "Focus Editor",
            keywords: &["sql", "query"],
            shortcut: Some("Alt+2"),
            disabled_reason: None,
            action: || Action::Focus(FocusTarget::Editor),
        },
        PaletteEntry {
            id: "focus.results",
            title: "Focus Results",
            keywords: &["grid", "rows"],
            shortcut: Some("Alt+3"),
            disabled_reason: None,
            action: || Action::Focus(FocusTarget::Results),
        },
        PaletteEntry {
            id: "focus.inspector",
            title: "Focus Inspector",
            keywords: &["details", "side"],
            shortcut: Some("Alt+4"),
            disabled_reason: None,
            action: || Action::Focus(FocusTarget::Inspector),
        },
        PaletteEntry {
            id: "layout.cycle",
            title: "Cycle Layout",
            keywords: &["preset", "panes", "split"],
            shortcut: Some("F10"),
            disabled_reason: None,
            action: || Action::CycleLayout,
        },
        PaletteEntry {
            id: "layout.results_focus",
            title: "Layout: Results focus",
            keywords: &["preset", "wide", "grid"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::LayoutResultsFocus,
        },
        PaletteEntry {
            id: "layout.hide_inspector",
            title: "Hide Inspector",
            keywords: &["layout", "pane", "toggle"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::HideInspector,
        },
        PaletteEntry {
            id: "layout.reset",
            title: "Reset layout",
            keywords: &["preset", "default", "panes"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ResetLayout,
        },
        PaletteEntry {
            id: "layout.results_grow",
            title: "Grow Results Pane",
            keywords: &["split", "height"],
            shortcut: Some("Alt+="),
            disabled_reason: None,
            action: || Action::GrowResults,
        },
        PaletteEntry {
            id: "layout.results_shrink",
            title: "Shrink Results Pane",
            keywords: &["split", "height"],
            shortcut: Some("Alt+-"),
            disabled_reason: None,
            action: || Action::ShrinkResults,
        },
        PaletteEntry {
            id: "layout.explorer_grow",
            title: "Grow Explorer Pane",
            keywords: &["split", "width"],
            shortcut: Some("Alt+]"),
            disabled_reason: None,
            action: || Action::GrowExplorer,
        },
        PaletteEntry {
            id: "layout.explorer_shrink",
            title: "Shrink Explorer Pane",
            keywords: &["split", "width"],
            shortcut: Some("Alt+["),
            disabled_reason: None,
            action: || Action::ShrinkExplorer,
        },
        PaletteEntry {
            id: "data.copy.csv",
            title: "Copy as CSV",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CopyGrid(dexo_app::data::CopyFormat::Csv),
        },
        PaletteEntry {
            id: "data.copy.text",
            title: "Copy as Text",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CopyGrid(dexo_app::data::CopyFormat::Text),
        },
        PaletteEntry {
            id: "data.copy.json",
            title: "Copy as JSON",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CopyGrid(dexo_app::data::CopyFormat::Json),
        },
        PaletteEntry {
            id: "data.copy.markdown",
            title: "Copy as Markdown",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CopyGrid(dexo_app::data::CopyFormat::Markdown),
        },
        PaletteEntry {
            id: "data.copy.sql",
            title: "Copy as SQL",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CopyGrid(dexo_app::data::CopyFormat::Sql),
        },
        PaletteEntry {
            id: "data.apply",
            title: "Apply Changes",
            keywords: &["mutate", "save"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ApplyChanges,
        },
        PaletteEntry {
            id: "data.revert",
            title: "Revert Changes",
            keywords: &["undo", "discard"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::RevertChanges,
        },
        PaletteEntry {
            id: "data.nav_back",
            title: "Data Navigate Back",
            keywords: &["crumb", "related"],
            shortcut: Some("b"),
            disabled_reason: None,
            action: || Action::DataNavBack,
        },
        PaletteEntry {
            id: "data.page_next",
            title: "Next Data Page",
            keywords: &["page", "offset"],
            shortcut: Some("n"),
            disabled_reason: None,
            action: || Action::NextDataPage,
        },
        PaletteEntry {
            id: "data.page_prev",
            title: "Previous Data Page",
            keywords: &["page", "offset"],
            shortcut: Some("p"),
            disabled_reason: None,
            action: || Action::PrevDataPage,
        },
        PaletteEntry {
            id: "data.sort",
            title: "Apply Remote Sort",
            keywords: &["order", "query"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ApplyRemoteSort,
        },
        PaletteEntry {
            id: "data.filter",
            title: "Apply Remote Filter",
            keywords: &["where", "query"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ApplyRemoteFilter,
        },
        PaletteEntry {
            id: "data.review",
            title: "Review Changes",
            keywords: &["apply", "edit"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenReview,
        },
        PaletteEntry {
            id: "data.related",
            title: "Open Related",
            keywords: &["foreign", "key"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenRelated,
        },
        PaletteEntry {
            id: "data.inspect",
            title: "Inspect Value",
            keywords: &["viewer", "json"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::InspectValue,
        },
        PaletteEntry {
            id: "schema.preview",
            title: "Preview DDL",
            keywords: &["schema", "ddl", "form"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenDdlPreview,
        },
        PaletteEntry {
            id: "schema.raw",
            title: "Apply Raw DDL",
            keywords: &["sql", "escape"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ApplyRawDdl,
        },
        PaletteEntry {
            id: "schema.diff",
            title: "Compare Schema",
            keywords: &["diff", "migration", "snapshot"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenSchemaDiff,
        },
        PaletteEntry {
            id: "transfer.export",
            title: "Export Data",
            keywords: &["csv", "json", "file"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenTransfer,
        },
        PaletteEntry {
            id: "transfer.import",
            title: "Import Data",
            keywords: &["csv", "json", "file"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenTransfer,
        },
        PaletteEntry {
            id: "backup.dump",
            title: "Native Backup",
            keywords: &["pg_dump", "mysqldump"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenBackup,
        },
        PaletteEntry {
            id: "backup.restore",
            title: "Native Restore",
            keywords: &["pg_restore", "mysql"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenRestore,
        },
        PaletteEntry {
            id: "schema.security",
            title: "Manage Grants",
            keywords: &["role", "user", "grant"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenSecurity,
        },
        PaletteEntry {
            id: "explain.open",
            title: "Explain Plan",
            keywords: &["analyze", "plan", "cost"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenExplain,
        },
        PaletteEntry {
            id: "admin.sessions",
            title: "Inspect Sessions",
            keywords: &["locks", "cancel", "terminate"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenAdmin,
        },
        PaletteEntry {
            id: "mcp.profiles",
            title: "MCP Profiles",
            keywords: &["mcp", "allowlist", "policy", "grant"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenMcpProfiles,
        },
        PaletteEntry {
            id: "explorer.expand",
            title: "Expand Explorer Node",
            keywords: &["tree", "open"],
            shortcut: Some("Enter"),
            disabled_reason: None,
            action: || Action::ExplorerExpand,
        },
        PaletteEntry {
            id: "explorer.refresh",
            title: "Refresh Catalog Node",
            keywords: &["reload", "tree"],
            shortcut: Some("r"),
            disabled_reason: None,
            action: || Action::RefreshCatalogNode,
        },
        PaletteEntry {
            id: "explorer.refresh_all",
            title: "Refresh Catalog",
            keywords: &["reload", "tree"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::RefreshCatalogAll,
        },
        PaletteEntry {
            id: "explorer.inspect",
            title: "Inspect Object",
            keywords: &["properties", "ddl"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenObjectInspector,
        },
        PaletteEntry {
            id: "explorer.ddl",
            title: "Open Object DDL",
            keywords: &["create", "script"],
            shortcut: Some("d"),
            disabled_reason: None,
            action: || Action::OpenObjectDdl,
        },
        PaletteEntry {
            id: "explorer.refresh_subtree",
            title: "Refresh Catalog Subtree",
            keywords: &["reload", "tree"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::RefreshCatalogSubtree,
        },
        PaletteEntry {
            id: "explorer.up",
            title: "Explorer Up",
            keywords: &["tree", "select"],
            shortcut: Some("Up"),
            disabled_reason: None,
            action: || Action::ExplorerUp,
        },
        PaletteEntry {
            id: "explorer.down",
            title: "Explorer Down",
            keywords: &["tree", "select"],
            shortcut: Some("Down"),
            disabled_reason: None,
            action: || Action::ExplorerDown,
        },
        PaletteEntry {
            id: "explorer.dependencies",
            title: "Show Dependencies",
            keywords: &["depends", "inspector"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenDependencies,
        },
        PaletteEntry {
            id: "explorer.dependents",
            title: "Show Dependents",
            keywords: &["used", "inspector"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenDependents,
        },
        PaletteEntry {
            id: "tab.sql",
            title: "Tab SQL",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+1"),
            disabled_reason: None,
            action: || Action::SwitchTab { index: 0 },
        },
        PaletteEntry {
            id: "tab.data",
            title: "Tab Data",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+2"),
            disabled_reason: None,
            action: || Action::SwitchTab { index: 1 },
        },
        PaletteEntry {
            id: "tab.ddl",
            title: "Tab DDL",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+3"),
            disabled_reason: None,
            action: || Action::SwitchTab { index: 2 },
        },
        PaletteEntry {
            id: "tab.properties",
            title: "Tab Properties",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+4"),
            disabled_reason: None,
            action: || Action::SwitchTab { index: 3 },
        },
        PaletteEntry {
            id: "tab.explain",
            title: "Tab Explain",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+5"),
            disabled_reason: None,
            action: || Action::SwitchTab { index: 4 },
        },
        PaletteEntry {
            id: "tab.next",
            title: "Next Tab",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+Tab"),
            disabled_reason: None,
            action: || Action::NextTab,
        },
        PaletteEntry {
            id: "document.next",
            title: "Next Document",
            keywords: &["editor", "tab"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::NextDocument,
        },
        PaletteEntry {
            id: "document.new",
            title: "New Document",
            keywords: &["editor", "scratch"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::NewDocument,
        },
        PaletteEntry {
            id: "document.save",
            title: "Save Document",
            keywords: &["file", "write"],
            shortcut: Some("Ctrl+S"),
            disabled_reason: None,
            action: || Action::SaveActiveDocument,
        },
        PaletteEntry {
            id: "document.open",
            title: "Open Document",
            keywords: &["file", "load"],
            shortcut: Some("Ctrl+O"),
            disabled_reason: None,
            action: || Action::OpenDocument,
        },
        PaletteEntry {
            id: "results.select_row",
            title: "Select Grid Row",
            keywords: &["grid"],
            shortcut: Some("r"),
            disabled_reason: None,
            action: || Action::SelectGridRow,
        },
        PaletteEntry {
            id: "results.select_column",
            title: "Select Grid Column",
            keywords: &["grid"],
            shortcut: Some("c"),
            disabled_reason: None,
            action: || Action::SelectGridColumn,
        },
        PaletteEntry {
            id: "results.next_tab",
            title: "Next Result Tab",
            keywords: &["grid"],
            shortcut: Some("]"),
            disabled_reason: None,
            action: || Action::NextResultTab,
        },
        PaletteEntry {
            id: "results.prev_tab",
            title: "Previous Result Tab",
            keywords: &["grid"],
            shortcut: Some("["),
            disabled_reason: None,
            action: || Action::PrevResultTab,
        },
        PaletteEntry {
            id: "inspector.next_tab",
            title: "Next Inspector Tab",
            keywords: &["ddl", "privileges"],
            shortcut: Some("Tab"),
            disabled_reason: None,
            action: || Action::InspectorNextTab,
        },
        PaletteEntry {
            id: "settings.theme",
            title: "Cycle Theme",
            keywords: &["dark", "light"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CycleTheme,
        },
        PaletteEntry {
            id: "settings.keymap",
            title: "Cycle Keymap",
            keywords: &["vim", "emacs"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CycleKeymap,
        },
        PaletteEntry {
            id: "settings.mouse",
            title: "Toggle Mouse",
            keywords: &["pointer"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ToggleMouse,
        },
        PaletteEntry {
            id: "explorer.data",
            title: "Open Object Data",
            keywords: &["rows", "table"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenObjectData,
        },
        PaletteEntry {
            id: "editor.goto",
            title: "Go To Definition",
            keywords: &["navigate", "catalog"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::GoToDefinition,
        },
        PaletteEntry {
            id: "explorer.copy_name",
            title: "Copy Object Name",
            keywords: &["clipboard", "tree"],
            shortcut: Some("c"),
            disabled_reason: None,
            action: || Action::ExplorerCopyName,
        },
        PaletteEntry {
            id: "explorer.copy_simple",
            title: "Copy Simple Name",
            keywords: &["clipboard", "tree"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CopySimpleName,
        },
        PaletteEntry {
            id: "explorer.copy_ddl",
            title: "Copy DDL",
            keywords: &["clipboard", "create"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CopyDdl,
        },
        PaletteEntry {
            id: "explorer.favorite",
            title: "Toggle Favorite",
            keywords: &["star", "pin"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ToggleFavorite,
        },
        PaletteEntry {
            id: "explorer.favorites_only",
            title: "Show Favorites Only",
            keywords: &["filter", "star"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ToggleFavoritesOnly,
        },
        PaletteEntry {
            id: "explorer.system_objects",
            title: "Toggle System Objects",
            keywords: &["filter", "pg_catalog", "mysql"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ToggleSystemObjects,
        },
        PaletteEntry {
            id: "results.up",
            title: "Results Up",
            keywords: &["grid", "scroll"],
            shortcut: Some("Up"),
            disabled_reason: None,
            action: || Action::ResultsUp,
        },
        PaletteEntry {
            id: "results.down",
            title: "Results Down",
            keywords: &["grid", "scroll"],
            shortcut: Some("Down"),
            disabled_reason: None,
            action: || Action::ResultsDown,
        },
        PaletteEntry {
            id: "results.left",
            title: "Results Left",
            keywords: &["grid", "scroll"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ResultsLeft,
        },
        PaletteEntry {
            id: "results.right",
            title: "Results Right",
            keywords: &["grid", "scroll"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ResultsRight,
        },
        PaletteEntry {
            id: "results.pageup",
            title: "Results Page Up",
            keywords: &["grid", "scroll"],
            shortcut: Some("PageUp"),
            disabled_reason: None,
            action: || Action::ResultsPageUp,
        },
        PaletteEntry {
            id: "results.pagedown",
            title: "Results Page Down",
            keywords: &["grid", "scroll"],
            shortcut: Some("PageDown"),
            disabled_reason: None,
            action: || Action::ResultsPageDown,
        },
        PaletteEntry {
            id: "results.top",
            title: "Results Top",
            keywords: &["grid", "home"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ResultsTop,
        },
        PaletteEntry {
            id: "results.extend_up",
            title: "Extend Results Selection Up",
            keywords: &["grid", "shift", "select"],
            shortcut: Some("Shift+Up"),
            disabled_reason: None,
            action: || Action::ResultsExtendUp,
        },
        PaletteEntry {
            id: "results.extend_down",
            title: "Extend Results Selection Down",
            keywords: &["grid", "shift", "select"],
            shortcut: Some("Shift+Down"),
            disabled_reason: None,
            action: || Action::ResultsExtendDown,
        },
        PaletteEntry {
            id: "results.actions",
            title: "Results Row Actions",
            keywords: &["grid", "copy", "menu"],
            shortcut: Some("Enter"),
            disabled_reason: None,
            action: || Action::OpenResultsMenu,
        },
        PaletteEntry {
            id: "connection.add",
            title: "Add Connection",
            keywords: &["database", "postgres", "mysql", "connect"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenConnectionForm,
        },
        PaletteEntry {
            id: "connection.browse",
            title: "Browse Connections",
            keywords: &["database", "sessions", "profiles"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenConnections,
        },
        PaletteEntry {
            id: "connection.connect",
            title: "Connect Selected",
            keywords: &["session"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ConnectSelected,
        },
        PaletteEntry {
            id: "connection.duplicate",
            title: "Duplicate Connection",
            keywords: &["copy", "profile"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::DuplicateConnection,
        },
        PaletteEntry {
            id: "connection.test",
            title: "Test Connection",
            keywords: &["ping"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::TestConnection,
        },
        PaletteEntry {
            id: "connection.delete",
            title: "Delete Connection",
            keywords: &["remove"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::DeleteConnection,
        },
        PaletteEntry {
            id: "connection.close_session",
            title: "Close Session",
            keywords: &["disconnect"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CloseSelectedSession,
        },
        PaletteEntry {
            id: "project.browse",
            title: "Browse Projects",
            keywords: &["workspace", "switch"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenProjects,
        },
        PaletteEntry {
            id: "project.switch",
            title: "Switch Project",
            keywords: &["workspace", "open"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::SwitchProject {
                name: String::new(),
            },
        },
        PaletteEntry {
            id: "project.create",
            title: "Create Project",
            keywords: &["workspace", "new"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CreateProject {
                name: String::new(),
            },
        },
        PaletteEntry {
            id: "project.rename",
            title: "Rename Project",
            keywords: &["workspace"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::RenameProject {
                name: String::new(),
            },
        },
        PaletteEntry {
            id: "project.delete",
            title: "Delete Project",
            keywords: &["workspace"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::DeleteProject,
        },
        PaletteEntry {
            id: "config.transfer",
            title: "Import/Export Config",
            keywords: &["portable", "toml"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenConfigTransfer,
        },
        PaletteEntry {
            id: "settings.open",
            title: "Open Settings",
            keywords: &["theme", "keymap", "mouse"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenSettings,
        },
        PaletteEntry {
            id: "settings.reset",
            title: "Reset Settings",
            keywords: &["defaults"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ConfirmResetSettings,
        },
        PaletteEntry {
            id: "recovery.open",
            title: "Session Recovery",
            keywords: &["crash", "restore"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenRecovery,
        },
        PaletteEntry {
            id: "recovery.restore",
            title: "Recover Session",
            keywords: &["crash"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ConfirmRecover,
        },
        PaletteEntry {
            id: "recovery.discard",
            title: "Discard Recovery",
            keywords: &["crash"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ConfirmDiscardRecovery,
        },
        PaletteEntry {
            id: "mcp.audit",
            title: "MCP Audit Log",
            keywords: &["mcp", "grant", "revoke"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenMcpAudit,
        },
        PaletteEntry {
            id: "mcp.revoke_all",
            title: "Revoke All MCP Grants",
            keywords: &["mcp", "grant"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::RevokeAllMcpGrants,
        },
        PaletteEntry {
            id: "editor.complete",
            title: "Trigger Completion",
            keywords: &["intellisense", "suggest"],
            shortcut: Some("Ctrl+Space"),
            disabled_reason: None,
            action: || Action::RefreshSqlIntelligence,
        },
        PaletteEntry {
            id: "editor.format",
            title: "Format SQL",
            keywords: &["pretty", "indent"],
            shortcut: Some("Ctrl+Shift+I"),
            disabled_reason: None,
            action: || Action::FormatSql,
        },
        PaletteEntry {
            id: "editor.accept_completion",
            title: "Accept Completion",
            keywords: &["complete"],
            shortcut: Some("Tab"),
            disabled_reason: None,
            action: || Action::AcceptCompletion,
        },
        PaletteEntry {
            id: "editor.snippet",
            title: "Insert Snippet",
            keywords: &["snippet", "template"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::InsertSnippet,
        },
        PaletteEntry {
            id: "editor.parameters",
            title: "Submit Parameters",
            keywords: &["bind", "params"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::SubmitParameters,
        },
        PaletteEntry {
            id: "editor.history",
            title: "Search History",
            keywords: &["rerun", "sql"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::SearchHistory,
        },
        PaletteEntry {
            id: "editor.history.clear",
            title: "Clear History",
            keywords: &["history", "delete"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ClearHistory,
        },
        PaletteEntry {
            id: "diagnostics.export",
            title: "Export Diagnostics",
            keywords: &["logs", "support"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenDiagnostics,
        },
    ]
}

pub fn results_menu_items() -> &'static [(&'static str, &'static str)] {
    &[
        ("copy-row-csv", "Copy row as CSV"),
        ("copy-cell", "Copy cell"),
        ("data.copy.json", "Copy as JSON"),
        ("data.copy.csv", "Copy as CSV"),
        ("data.copy.markdown", "Copy as Markdown"),
        ("data.copy.sql", "Copy as SQL"),
        ("data.inspect", "Inspect value"),
        ("data.filter", "Apply remote filter"),
        ("data.related", "Open related"),
    ]
}

pub fn action_by_id(id: &str) -> Option<Action> {
    palette_entries(&Model::default())
        .into_iter()
        .find(|entry| entry.id == id)
        .map(|entry| (entry.action)())
}

/// Popup list rows for a terminal height. Matches `render_palette` (height clamp 5..=12, minus border+query).
pub fn popup_list_rows(term_height: u16) -> usize {
    term_height.clamp(5, 12).saturating_sub(3) as usize
}

/// Keep `selected` inside `[offset, offset + rows)`. Same rule as ratatui `ListState`.
pub fn scroll_to_selection(selected: usize, offset: usize, count: usize, rows: usize) -> usize {
    if count == 0 || rows == 0 {
        return 0;
    }
    let selected = selected.min(count - 1);
    let max_offset = count.saturating_sub(rows);
    if selected < offset {
        selected
    } else if selected >= offset.saturating_add(rows) {
        selected
            .saturating_add(1)
            .saturating_sub(rows)
            .min(max_offset)
    } else {
        offset.min(max_offset)
    }
}

pub fn filter_entries<'a>(entries: &'a [PaletteEntry], query: &str) -> Vec<&'a PaletteEntry> {
    if query.is_empty() {
        return entries.iter().collect();
    }
    let mut scored: Vec<(u8, &PaletteEntry)> = entries
        .iter()
        .filter_map(|entry| score(entry, query).map(|s| (s, entry)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(b.1.title)));
    scored.into_iter().map(|(_, entry)| entry).collect()
}

fn score(entry: &PaletteEntry, query: &str) -> Option<u8> {
    let query = query.to_ascii_lowercase();
    let haystacks = std::iter::once(entry.title)
        .chain(entry.keywords.iter().copied())
        .chain(std::iter::once(entry.id));
    haystacks.filter_map(|text| score_text(text, &query)).max()
}

fn score_text(text: &str, query: &str) -> Option<u8> {
    let text = text.to_ascii_lowercase();
    if text.starts_with(query) {
        return Some(3);
    }
    if text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.starts_with(query))
    {
        return Some(2);
    }
    if is_subsequence(&text, query) {
        return Some(1);
    }
    None
}

fn is_subsequence(text: &str, query: &str) -> bool {
    let mut chars = text.chars();
    query.chars().all(|needle| chars.any(|ch| ch == needle))
}

#[cfg(test)]
mod tests {
    use super::{filter_entries, palette_entries, popup_list_rows, scroll_to_selection};
    use crate::model::Model;
    use dexo_driver_api::TransactionState;

    #[test]
    fn palette_explains_disabled_commit() {
        let entries = palette_entries(&Model::fixture(TransactionState::Idle));
        let commit = entries
            .iter()
            .find(|e| e.id == "transaction.commit")
            .unwrap();
        assert_eq!(commit.disabled_reason, Some("no active transaction"));
    }

    #[test]
    fn fuzzy_prefers_prefix_over_subsequence() {
        let entries = palette_entries(&Model::default());
        let filtered = filter_entries(&entries, "quit");
        assert_eq!(filtered[0].id, "workbench.quit");
    }

    #[test]
    fn fuzzy_word_start_beats_subsequence() {
        let entries = palette_entries(&Model::default());
        let filtered = filter_entries(&entries, "pal");
        assert_eq!(filtered[0].id, "palette.open");
    }

    #[test]
    fn scroll_keeps_selection_in_window() {
        assert_eq!(scroll_to_selection(0, 0, 20, 9), 0);
        assert_eq!(scroll_to_selection(8, 0, 20, 9), 0);
        assert_eq!(scroll_to_selection(9, 0, 20, 9), 1);
        assert_eq!(scroll_to_selection(8, 1, 20, 9), 1);
        assert_eq!(scroll_to_selection(0, 1, 20, 9), 0);
        assert_eq!(scroll_to_selection(19, 1, 20, 9), 11);

        let mut model = Model::default();
        model.palette.open = true;
        let entries = palette_entries(&model);
        model.palette.selected = entries.len() - 1;
        model.palette.offset = scroll_to_selection(
            model.palette.selected,
            0,
            entries.len(),
            popup_list_rows(model.height),
        );
        let view = crate::render::render_to_string(&model, 80, 24);
        let last = entries.last().unwrap().title;
        assert!(
            view.contains(last),
            "selected command `{last}` should stay visible after scroll"
        );
        assert!(
            !view.contains(entries[0].title),
            "first command should scroll off when selection is at the end"
        );
    }

    #[test]
    fn every_current_action_is_in_palette() {
        let ids: Vec<_> = palette_entries(&Model::default())
            .iter()
            .map(|entry| entry.id)
            .collect();
        for id in [
            "workbench.quit",
            "palette.open",
            "query.execute",
            "query.cancel",
            "transaction.commit",
            "transaction.rollback",
            "focus.explorer",
            "focus.editor",
            "focus.results",
            "focus.inspector",
            "help.open",
            "layout.cycle",
        ] {
            assert!(ids.contains(&id), "missing {id}");
        }
    }

    #[test]
    fn help_layout_and_results_menu_actions() {
        use crate::action::{Action, FocusTarget};
        use crate::layout::LayoutPreset;
        use crate::model::GridSelection;
        use crate::update::update;

        let mut model = Model::default();
        update(&mut model, Action::ToggleHelp);
        assert!(model.help.open);
        let view = crate::render::render_to_string(&model, 100, 40);
        assert!(view.contains("Keybindings"));
        assert!(view.contains("Editor"));
        update(&mut model, Action::ToggleHelp);
        assert!(!model.help.open);

        update(&mut model, Action::CycleLayout);
        assert_eq!(model.layout_preset, LayoutPreset::ResultsWide);
        assert!(!model.panes.inspector_visible);
        update(&mut model, Action::ResetLayout);
        assert_eq!(model.layout_preset, LayoutPreset::Normal);
        assert!(model.panes.inspector_visible);

        update(&mut model, Action::Focus(FocusTarget::Results));
        model.results = crate::model::ResultsState::default();
        *model.results = crate::model::GridModel::sample_rows(6);
        update(&mut model, Action::ResultsDown);
        assert_eq!(model.results.cursor_row(), Some(1));
        update(&mut model, Action::ResultsExtendDown);
        assert!(matches!(
            model.results.kind,
            GridSelection::Range {
                start: (1, _),
                end: (2, _)
            }
        ));
        update(&mut model, Action::OpenResultsMenu);
        assert!(model.results_menu.open);
        let view = crate::render::render_to_string(&model, 80, 24);
        assert!(view.contains("Row actions"));
        update(
            &mut model,
            Action::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            )),
        );
        assert!(!model.results_menu.open);

        update(&mut model, Action::Focus(FocusTarget::Editor));
        let view = crate::render::render_to_string(&model, 100, 40);
        assert!(view.contains("FOCUS: Editor"));
        assert!(view.contains("▸ SQL") || view.contains("> SQL"));
    }
}
