use dexo_driver_api::TransactionState;

use super::{CommandSpec, FlowIntent, PaletteEntry, PaletteInvocation, Requirement};
use crate::action::{Action, FocusTarget};
use crate::model::{GridSelection, Model};

fn command_spec_list() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "workbench.quit",
            title: "Quit",
            keywords: &["exit", "close"],
            shortcut: Some("Ctrl+Q"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::Quit),
        },
        CommandSpec {
            id: "palette.open",
            title: "Command Palette",
            keywords: &["commands", "search"],
            shortcut: Some("Ctrl+P"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenPalette),
        },
        CommandSpec {
            id: "query.execute_statement",
            title: "Execute Statement",
            keywords: &["run", "sql", "cursor"],
            shortcut: Some("Ctrl+Enter"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ExecuteStatement),
        },
        CommandSpec {
            id: "query.execute_selection",
            title: "Execute Selection",
            keywords: &["run", "sql", "selected"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ExecuteSelection),
        },
        CommandSpec {
            id: "query.execute_document",
            title: "Execute Document",
            keywords: &["run", "sql", "all"],
            shortcut: Some("Ctrl+Shift+F10"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ExecuteDocument),
        },
        CommandSpec {
            id: "query.cancel",
            title: "Cancel Query",
            keywords: &["stop", "abort"],
            shortcut: Some("Ctrl+F2"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CancelQuery),
        },
        CommandSpec {
            id: "transaction.begin",
            title: "Begin Transaction",
            keywords: &["tx", "begin", "start"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::BeginTransaction),
        },
        CommandSpec {
            id: "transaction.savepoint",
            title: "Create Savepoint",
            keywords: &["tx", "savepoint"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::SavepointCreate),
        },
        CommandSpec {
            id: "transaction.rollback_savepoint",
            title: "Rollback Savepoint",
            keywords: &["tx", "savepoint", "rollback"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::SavepointRollback),
        },
        CommandSpec {
            id: "transaction.release_savepoint",
            title: "Release Savepoint",
            keywords: &["tx", "savepoint", "release"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::SavepointRelease),
        },
        CommandSpec {
            id: "transaction.commit",
            title: "Commit Transaction",
            keywords: &["tx", "commit"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CommitTransaction),
        },
        CommandSpec {
            id: "transaction.rollback",
            title: "Rollback Transaction",
            keywords: &["tx", "abort"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::RollbackTransaction),
        },
        CommandSpec {
            id: "help.open",
            title: "Show Keybindings",
            keywords: &["help", "keys", "cheatsheet", "shortcuts"],
            shortcut: Some("F1"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ToggleHelp),
        },
        CommandSpec {
            id: "focus.explorer",
            title: "Focus Explorer",
            keywords: &["sidebar", "tree"],
            shortcut: Some("Alt+1"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::Focus(FocusTarget::Explorer)),
        },
        CommandSpec {
            id: "focus.editor",
            title: "Focus Editor",
            keywords: &["sql", "query"],
            shortcut: Some("Alt+2"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::Focus(FocusTarget::Editor)),
        },
        CommandSpec {
            id: "focus.results",
            title: "Focus Results",
            keywords: &["grid", "rows"],
            shortcut: Some("Alt+3"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::Focus(FocusTarget::Results)),
        },
        CommandSpec {
            id: "focus.inspector",
            title: "Focus Inspector",
            keywords: &["details", "side"],
            shortcut: Some("Alt+4"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::Focus(FocusTarget::Inspector)),
        },
        CommandSpec {
            id: "layout.cycle",
            title: "Cycle Layout",
            keywords: &["preset", "panes", "split"],
            shortcut: Some("F10"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CycleLayout),
        },
        CommandSpec {
            id: "layout.results_focus",
            title: "Layout: Results focus",
            keywords: &["preset", "wide", "grid"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::LayoutResultsFocus),
        },
        CommandSpec {
            id: "layout.hide_inspector",
            title: "Hide Inspector",
            keywords: &["layout", "pane", "toggle"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::HideInspector),
        },
        CommandSpec {
            id: "layout.reset",
            title: "Reset layout",
            keywords: &["preset", "default", "panes"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ResetLayout),
        },
        CommandSpec {
            id: "layout.results_grow",
            title: "Grow Results Pane",
            keywords: &["split", "height"],
            shortcut: Some("Alt+="),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::GrowResults),
        },
        CommandSpec {
            id: "layout.results_shrink",
            title: "Shrink Results Pane",
            keywords: &["split", "height"],
            shortcut: Some("Alt+-"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ShrinkResults),
        },
        CommandSpec {
            id: "layout.explorer_grow",
            title: "Grow Explorer Pane",
            keywords: &["split", "width"],
            shortcut: Some("Alt+]"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::GrowExplorer),
        },
        CommandSpec {
            id: "layout.explorer_shrink",
            title: "Shrink Explorer Pane",
            keywords: &["split", "width"],
            shortcut: Some("Alt+["),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ShrinkExplorer),
        },
        CommandSpec {
            id: "layout.inspector_grow",
            title: "Grow Inspector Pane",
            keywords: &["split", "width"],
            shortcut: Some("Alt+="),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::GrowInspector),
        },
        CommandSpec {
            id: "layout.inspector_shrink",
            title: "Shrink Inspector Pane",
            keywords: &["split", "width"],
            shortcut: Some("Alt+-"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ShrinkInspector),
        },
        CommandSpec {
            id: "data.copy.csv",
            title: "Copy as CSV",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CopyGrid(
                dexo_app::data::CopyFormat::Csv,
            )),
        },
        CommandSpec {
            id: "data.copy.text",
            title: "Copy as Text",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CopyGrid(
                dexo_app::data::CopyFormat::Text,
            )),
        },
        CommandSpec {
            id: "data.copy.json",
            title: "Copy as JSON",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CopyGrid(
                dexo_app::data::CopyFormat::Json,
            )),
        },
        CommandSpec {
            id: "data.copy.markdown",
            title: "Copy as Markdown",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CopyGrid(
                dexo_app::data::CopyFormat::Markdown,
            )),
        },
        CommandSpec {
            id: "data.copy.sql",
            title: "Copy as SQL",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CopyGrid(
                dexo_app::data::CopyFormat::Sql,
            )),
        },
        CommandSpec {
            id: "data.apply",
            title: "Apply Changes",
            keywords: &["mutate", "save"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ApplyChanges),
        },
        CommandSpec {
            id: "data.revert",
            title: "Revert Changes",
            keywords: &["undo", "discard"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::RevertChanges),
        },
        CommandSpec {
            id: "data.nav_back",
            title: "Data Navigate Back",
            keywords: &["crumb", "related"],
            shortcut: Some("b"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::DataNavBack),
        },
        CommandSpec {
            id: "data.page_next",
            title: "Next Data Page",
            keywords: &["page", "offset"],
            shortcut: Some("n"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::NextDataPage),
        },
        CommandSpec {
            id: "data.page_prev",
            title: "Previous Data Page",
            keywords: &["page", "offset"],
            shortcut: Some("p"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::PrevDataPage),
        },
        CommandSpec {
            id: "data.sort",
            title: "Apply Remote Sort",
            keywords: &["order", "query"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::DataSort),
        },
        CommandSpec {
            id: "data.filter",
            title: "Apply Remote Filter",
            keywords: &["where", "query"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::DataFilter),
        },
        CommandSpec {
            id: "data.review",
            title: "Review Changes",
            keywords: &["apply", "edit"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::DataReview),
        },
        CommandSpec {
            id: "data.related",
            title: "Open Related",
            keywords: &["foreign", "key"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenRelated),
        },
        CommandSpec {
            id: "data.inspect",
            title: "Inspect Value",
            keywords: &["viewer", "json"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::InspectValue),
        },
        CommandSpec {
            id: "schema.preview",
            title: "Preview DDL",
            keywords: &["schema", "ddl", "form"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::SchemaPreview),
        },
        CommandSpec {
            id: "schema.raw",
            title: "Apply Raw DDL",
            keywords: &["sql", "escape"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::SchemaRaw),
        },
        CommandSpec {
            id: "schema.diff",
            title: "Compare Schema",
            keywords: &["diff", "migration", "snapshot"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::SchemaDiff),
        },
        CommandSpec {
            id: "transfer.export",
            title: "Export Data",
            keywords: &["csv", "json", "file"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::TransferExport),
        },
        CommandSpec {
            id: "transfer.import",
            title: "Import Data",
            keywords: &["csv", "json", "file"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::TransferImport),
        },
        CommandSpec {
            id: "backup.dump",
            title: "Native Backup",
            keywords: &["pg_dump", "mysqldump"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::Backup),
        },
        CommandSpec {
            id: "backup.restore",
            title: "Native Restore",
            keywords: &["pg_restore", "mysql"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::Restore),
        },
        CommandSpec {
            id: "schema.security",
            title: "Manage Grants",
            keywords: &["role", "user", "grant"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::Security),
        },
        CommandSpec {
            id: "explain.open",
            title: "Explain Plan",
            keywords: &["analyze", "plan", "cost"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenExplain),
        },
        CommandSpec {
            id: "admin.sessions",
            title: "Inspect Sessions",
            keywords: &["locks", "cancel", "terminate"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenAdmin),
        },
        CommandSpec {
            id: "mcp.profiles",
            title: "MCP Profiles",
            keywords: &["mcp", "allowlist", "policy", "grant"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenMcpProfiles),
        },
        CommandSpec {
            id: "explorer.expand",
            title: "Activate Sidebar Selection",
            keywords: &["tree", "connection", "connect", "open", "enter", "table"],
            shortcut: Some("Enter"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ExplorerExpand),
        },
        CommandSpec {
            id: "connection.new",
            title: "New Connection",
            keywords: &["database", "profile", "connect"],
            shortcut: Some("n"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenConnectionForm),
        },
        CommandSpec {
            id: "connection.edit",
            title: "Edit Selected Connection",
            keywords: &["database", "profile", "connect"],
            shortcut: Some("e"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditSelectedConnection),
        },
        CommandSpec {
            id: "explorer.refresh",
            title: "Refresh Catalog Node",
            keywords: &["reload", "tree"],
            shortcut: Some("r"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::RefreshCatalogNode),
        },
        CommandSpec {
            id: "explorer.refresh_all",
            title: "Refresh Catalog",
            keywords: &["reload", "tree"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::RefreshCatalogAll),
        },
        CommandSpec {
            id: "explorer.inspect",
            title: "Inspect Object",
            keywords: &["properties", "ddl"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenObjectInspector),
        },
        CommandSpec {
            id: "explorer.ddl",
            title: "Open Object DDL",
            keywords: &["create", "script"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenObjectDdl),
        },
        CommandSpec {
            id: "explorer.refresh_subtree",
            title: "Refresh Catalog Subtree",
            keywords: &["reload", "tree"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::RefreshCatalogSubtree),
        },
        CommandSpec {
            id: "explorer.up",
            title: "Explorer Up",
            keywords: &["tree", "select"],
            shortcut: Some("Up"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ExplorerUp),
        },
        CommandSpec {
            id: "explorer.down",
            title: "Explorer Down",
            keywords: &["tree", "select"],
            shortcut: Some("Down"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ExplorerDown),
        },
        CommandSpec {
            id: "explorer.dependencies",
            title: "Show Dependencies",
            keywords: &["depends", "inspector"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenDependencies),
        },
        CommandSpec {
            id: "explorer.dependents",
            title: "Show Dependents",
            keywords: &["used", "inspector"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenDependents),
        },
        CommandSpec {
            id: "tab.sql",
            title: "Tab SQL",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+1"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::SwitchTab { index: 0 }),
        },
        CommandSpec {
            id: "tab.data",
            title: "Tab Data",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+2"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::SwitchTab { index: 1 }),
        },
        CommandSpec {
            id: "tab.ddl",
            title: "Tab DDL",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+3"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::SwitchTab { index: 2 }),
        },
        CommandSpec {
            id: "tab.properties",
            title: "Tab Properties",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+4"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::SwitchTab { index: 3 }),
        },
        CommandSpec {
            id: "tab.explain",
            title: "Tab Explain",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+5"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::SwitchTab { index: 4 }),
        },
        CommandSpec {
            id: "tab.next",
            title: "Next Tab",
            keywords: &["workbench"],
            shortcut: Some("Ctrl+Tab"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::NextTab),
        },
        CommandSpec {
            id: "document.next",
            title: "Next Document",
            keywords: &["editor", "tab"],
            shortcut: Some("Ctrl+Tab"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::NextDocument),
        },
        CommandSpec {
            id: "document.prev",
            title: "Previous Document",
            keywords: &["editor", "tab"],
            shortcut: Some("Ctrl+Shift+Tab"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::PrevDocument),
        },
        CommandSpec {
            id: "document.close",
            title: "Close Document",
            keywords: &["editor", "tab"],
            shortcut: Some("Ctrl+W"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CloseDocument),
        },
        CommandSpec {
            id: "document.new",
            title: "New Document",
            keywords: &["editor", "scratch"],
            shortcut: Some("Ctrl+N"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::NewDocument),
        },
        CommandSpec {
            id: "document.save",
            title: "Save Document",
            keywords: &["file", "write"],
            shortcut: Some("Ctrl+S"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::SaveActiveDocument),
        },
        CommandSpec {
            id: "document.open",
            title: "Open Document",
            keywords: &["file", "load"],
            shortcut: Some("Ctrl+O"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenDocument),
        },
        CommandSpec {
            id: "results.select_row",
            title: "Select Grid Row",
            keywords: &["grid"],
            shortcut: Some("r"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::SelectGridRow),
        },
        CommandSpec {
            id: "results.select_column",
            title: "Select Grid Column",
            keywords: &["grid"],
            shortcut: Some("c"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::SelectGridColumn),
        },
        CommandSpec {
            id: "results.next_tab",
            title: "Next Result Tab",
            keywords: &["grid"],
            shortcut: Some("]"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::NextResultTab),
        },
        CommandSpec {
            id: "results.prev_tab",
            title: "Previous Result Tab",
            keywords: &["grid"],
            shortcut: Some("["),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::PrevResultTab),
        },
        CommandSpec {
            id: "inspector.next_tab",
            title: "Next Inspector Tab",
            keywords: &["ddl", "privileges"],
            shortcut: Some("Tab"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::InspectorNextTab),
        },
        CommandSpec {
            id: "settings.theme",
            title: "Cycle Theme",
            keywords: &["dark", "light"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CycleTheme),
        },
        CommandSpec {
            id: "settings.keymap",
            title: "Cycle Keymap",
            keywords: &["vim", "emacs"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CycleKeymap),
        },
        CommandSpec {
            id: "settings.mouse",
            title: "Toggle Mouse",
            keywords: &["pointer"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ToggleMouse),
        },
        CommandSpec {
            id: "explorer.data",
            title: "Open Object Data",
            keywords: &["rows", "table"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenObjectData),
        },
        CommandSpec {
            id: "editor.goto",
            title: "Go To Definition",
            keywords: &["navigate", "catalog"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::GoToDefinition),
        },
        CommandSpec {
            id: "explorer.copy_name",
            title: "Copy Object Name",
            keywords: &["clipboard", "tree"],
            shortcut: Some("c"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ExplorerCopyName),
        },
        CommandSpec {
            id: "explorer.copy_simple",
            title: "Copy Simple Name",
            keywords: &["clipboard", "tree"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CopySimpleName),
        },
        CommandSpec {
            id: "explorer.copy_ddl",
            title: "Copy DDL",
            keywords: &["clipboard", "create"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CopyDdl),
        },
        CommandSpec {
            id: "explorer.favorite",
            title: "Toggle Favorite",
            keywords: &["star", "pin"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ToggleFavorite),
        },
        CommandSpec {
            id: "explorer.favorites_only",
            title: "Show Favorites Only",
            keywords: &["filter", "star"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ToggleFavoritesOnly),
        },
        CommandSpec {
            id: "explorer.system_objects",
            title: "Toggle System Objects",
            keywords: &["filter", "pg_catalog", "mysql"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ToggleSystemObjects),
        },
        CommandSpec {
            id: "results.up",
            title: "Results Up",
            keywords: &["grid", "scroll"],
            shortcut: Some("Up"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ResultsUp),
        },
        CommandSpec {
            id: "results.down",
            title: "Results Down",
            keywords: &["grid", "scroll"],
            shortcut: Some("Down"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ResultsDown),
        },
        CommandSpec {
            id: "results.left",
            title: "Results Left",
            keywords: &["grid", "scroll", "columns"],
            shortcut: Some("Left"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ResultsLeft),
        },
        CommandSpec {
            id: "results.right",
            title: "Results Right",
            keywords: &["grid", "scroll", "columns"],
            shortcut: Some("Right"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ResultsRight),
        },
        CommandSpec {
            id: "results.pageup",
            title: "Results Page Up",
            keywords: &["grid", "scroll"],
            shortcut: Some("PageUp"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ResultsPageUp),
        },
        CommandSpec {
            id: "results.pagedown",
            title: "Results Page Down",
            keywords: &["grid", "scroll"],
            shortcut: Some("PageDown"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ResultsPageDown),
        },
        CommandSpec {
            id: "results.top",
            title: "Results Top",
            keywords: &["grid", "home"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ResultsTop),
        },
        CommandSpec {
            id: "results.extend_up",
            title: "Extend Results Selection Up",
            keywords: &["grid", "shift", "select"],
            shortcut: Some("Shift+Up"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ResultsExtendUp),
        },
        CommandSpec {
            id: "results.extend_down",
            title: "Extend Results Selection Down",
            keywords: &["grid", "shift", "select"],
            shortcut: Some("Shift+Down"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ResultsExtendDown),
        },
        CommandSpec {
            id: "results.actions",
            title: "Results Row Actions",
            keywords: &["grid", "copy", "menu"],
            shortcut: Some("Enter"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenResultsMenu),
        },
        CommandSpec {
            id: "results.toggle_pick",
            title: "Toggle Results Row Pick",
            keywords: &["grid", "ctrl", "select"],
            shortcut: Some("Ctrl+Enter"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ToggleResultsPick),
        },
        CommandSpec {
            id: "connection.add",
            title: "Add Connection",
            keywords: &["database", "postgres", "mysql", "connect"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenConnectionForm),
        },
        CommandSpec {
            id: "connection.browse",
            title: "Browse Connections",
            keywords: &["database", "sessions", "profiles"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenConnections),
        },
        CommandSpec {
            id: "connection.connect",
            title: "Connect / Switch Session",
            keywords: &["session", "switch"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::ConnectionConnect),
        },
        CommandSpec {
            id: "connection.duplicate",
            title: "Duplicate Connection",
            keywords: &["copy", "profile"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::ConnectionDuplicate),
        },
        CommandSpec {
            id: "connection.test",
            title: "Test Connection",
            keywords: &["ping"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::ConnectionTest),
        },
        CommandSpec {
            id: "connection.delete",
            title: "Delete Connection",
            keywords: &["remove"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::ConnectionDelete),
        },
        CommandSpec {
            id: "connection.close_session",
            title: "Disconnect Connection",
            keywords: &["close", "session"],
            shortcut: Some("Shift+D"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CloseSelectedSession),
        },
        CommandSpec {
            id: "project.browse",
            title: "Browse Projects",
            keywords: &["workspace", "switch"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenProjects),
        },
        CommandSpec {
            id: "project.switch",
            title: "Switch Project",
            keywords: &["workspace", "open"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::ProjectSwitch),
        },
        CommandSpec {
            id: "project.create",
            title: "Create Project",
            keywords: &["workspace", "new"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::ProjectCreate),
        },
        CommandSpec {
            id: "project.rename",
            title: "Rename Project",
            keywords: &["workspace"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::ProjectRename),
        },
        CommandSpec {
            id: "project.delete",
            title: "Delete Project",
            keywords: &["workspace"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::ProjectDelete),
        },
        CommandSpec {
            id: "config.transfer",
            title: "Import/Export Config",
            keywords: &["portable", "toml"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenConfigTransfer),
        },
        CommandSpec {
            id: "settings.open",
            title: "Open Settings",
            keywords: &["theme", "keymap", "mouse"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenSettings),
        },
        CommandSpec {
            id: "settings.reset",
            title: "Reset Settings",
            keywords: &["defaults"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::SettingsReset),
        },
        CommandSpec {
            id: "recovery.open",
            title: "Session Recovery",
            keywords: &["crash", "restore"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenRecovery),
        },
        CommandSpec {
            id: "recovery.restore",
            title: "Recover Session",
            keywords: &["crash"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::RecoveryRestore),
        },
        CommandSpec {
            id: "recovery.discard",
            title: "Discard Recovery",
            keywords: &["crash"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::RecoveryDiscard),
        },
        CommandSpec {
            id: "mcp.audit",
            title: "MCP Audit Log",
            keywords: &["mcp", "grant", "revoke"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenMcpAudit),
        },
        CommandSpec {
            id: "mcp.revoke_all",
            title: "Revoke All MCP Grants",
            keywords: &["mcp", "grant"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::McpRevokeAll),
        },
        CommandSpec {
            id: "editor.complete",
            title: "Trigger Completion",
            keywords: &["intellisense", "suggest"],
            shortcut: Some("Ctrl+Space"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::RefreshSqlIntelligence),
        },
        CommandSpec {
            id: "editor.format",
            title: "Format SQL",
            keywords: &["pretty", "indent"],
            shortcut: Some("Ctrl+Shift+I"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::FormatSql),
        },
        CommandSpec {
            id: "editor.accept_completion",
            title: "Accept Completion",
            keywords: &["complete"],
            shortcut: Some("Tab"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::AcceptCompletion),
        },
        CommandSpec {
            id: "editor.snippet",
            title: "Insert Snippet",
            keywords: &["snippet", "template"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::InsertSnippet),
        },
        CommandSpec {
            id: "editor.parameters",
            title: "Submit Parameters",
            keywords: &["bind", "params"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::SubmitParameters),
        },
        CommandSpec {
            id: "editor.history",
            title: "Search History",
            keywords: &["rerun", "sql"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::SearchHistory),
        },
        CommandSpec {
            id: "editor.history.clear",
            title: "Clear History",
            keywords: &["history", "delete"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::ClearHistory),
        },
        CommandSpec {
            id: "diagnostics.export",
            title: "Export Diagnostics",
            keywords: &["logs", "support"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::OpenFlow(FlowIntent::DiagnosticsExport),
        },
    ]
}

pub fn command_specs() -> Vec<CommandSpec> {
    command_spec_list()
        .into_iter()
        .map(|mut spec| {
            spec.requirements = requirements_for(spec.id);
            spec
        })
        .collect()
}

pub fn command_spec(id: &str) -> Option<CommandSpec> {
    command_specs().into_iter().find(|spec| spec.id == id)
}

pub fn palette_entries(model: &Model) -> Vec<PaletteEntry> {
    command_specs()
        .into_iter()
        .map(|spec| PaletteEntry {
            id: spec.id,
            title: spec.title,
            keywords: spec.keywords,
            shortcut: spec.shortcut,
            requirements: spec.requirements,
            disabled_reason: first_unmet(model, spec.requirements)
                .or_else(|| contextual_reason(model, spec.id)),
            invocation: spec.invocation,
        })
        .collect()
}

fn unmet_requirement(model: &Model, requirement: Requirement) -> Option<String> {
    let unmet = match requirement {
        Requirement::ActiveSession => model.active_session.is_none(),
        Requirement::Results => model.results.rows().is_empty(),
        Requirement::RowSelection => matches!(model.results.kind, GridSelection::Column { .. }),
        Requirement::ExplorerNode => model.explorer.selected.is_none(),
        Requirement::LoadedDdl => model.inspector.ddl.is_none(),
        Requirement::PendingChanges => model.data.changes.pending().is_empty(),
        Requirement::Breadcrumb => model.data.crumbs.is_empty(),
        Requirement::ActiveQuery => model.active_operation.is_none(),
        Requirement::Completion => model.editor.completions.is_empty(),
        Requirement::Parameters => model.editor.parameters.is_empty(),
        Requirement::History => model.editor.history.is_empty(),
        Requirement::Recovery => model.recovery.checkpoints.is_empty(),
    };
    unmet.then(|| requirement.reason().to_string())
}

fn first_unmet(model: &Model, requirements: &[Requirement]) -> Option<String> {
    requirements
        .iter()
        .find_map(|value| unmet_requirement(model, *value))
}

fn contextual_reason(model: &Model, id: &str) -> Option<String> {
    if model.connection.read_only
        && matches!(
            id,
            "transaction.begin"
                | "transaction.savepoint"
                | "transaction.rollback_savepoint"
                | "transaction.release_savepoint"
                | "transaction.commit"
                | "transaction.rollback"
        )
    {
        return Some("connection is read-only".into());
    }
    match id {
        "transaction.begin" if model.transaction != TransactionState::Idle => {
            Some("session is not idle".into())
        }
        "transaction.savepoint" | "transaction.release_savepoint" | "transaction.commit"
            if model.transaction != TransactionState::Active =>
        {
            Some("no active transaction".into())
        }
        "transaction.rollback_savepoint" | "transaction.rollback"
            if !matches!(
                model.transaction,
                TransactionState::Active | TransactionState::Failed
            ) =>
        {
            Some("no active transaction".into())
        }
        _ => None,
    }
}

fn requirements_for(id: &str) -> &'static [Requirement] {
    use Requirement::*;
    match id {
        "query.execute_statement"
        | "query.execute_selection"
        | "query.execute_document"
        | "transaction.begin"
        | "transaction.savepoint"
        | "transaction.rollback_savepoint"
        | "transaction.release_savepoint"
        | "transaction.commit"
        | "transaction.rollback"
        | "schema.preview"
        | "schema.raw"
        | "schema.diff"
        | "schema.security"
        | "explain.open"
        | "admin.sessions"
        | "data.page_next"
        | "data.page_prev" => &[ActiveSession],
        "explorer.inspect"
        | "explorer.ddl"
        | "explorer.dependencies"
        | "explorer.dependents"
        | "explorer.data" => &[ActiveSession, ExplorerNode],
        "data.sort" | "data.filter" => &[ActiveSession, Results],
        "data.apply" => &[ActiveSession, PendingChanges],
        "data.copy.csv"
        | "data.copy.text"
        | "data.copy.json"
        | "data.copy.markdown"
        | "data.copy.sql"
        | "transfer.export"
        | "results.select_row"
        | "results.select_column"
        | "results.next_tab"
        | "results.prev_tab"
        | "results.up"
        | "results.down"
        | "results.left"
        | "results.right"
        | "results.pageup"
        | "results.pagedown"
        | "results.top"
        | "results.extend_up"
        | "results.extend_down" => &[Results],
        "data.inspect" | "data.related" | "results.actions" | "results.toggle_pick" => {
            &[Results, RowSelection]
        }
        "explorer.expand"
        | "explorer.refresh_subtree"
        | "explorer.copy_name"
        | "explorer.copy_simple"
        | "explorer.favorite"
        | "explorer.up"
        | "explorer.down" => &[ExplorerNode],
        "explorer.copy_ddl" => &[LoadedDdl],
        "data.revert" | "data.review" => &[PendingChanges],
        "data.nav_back" => &[Breadcrumb],
        "query.cancel" => &[ActiveQuery],
        "editor.accept_completion" => &[Completion],
        "editor.parameters" => &[Parameters],
        "editor.history.clear" => &[History],
        "recovery.restore" | "recovery.discard" => &[Recovery],
        _ => &[],
    }
}
