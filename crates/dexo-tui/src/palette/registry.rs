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
            id: "focus.tabs",
            title: "Focus Document Tabs",
            keywords: &["strip", "files", "documents"],
            shortcut: Some("Alt+0"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::Focus(FocusTarget::DocumentTabs)),
        },
        CommandSpec {
            id: "document.tab_prev",
            title: "Previous Tab In Strip",
            keywords: &["strip", "cursor"],
            shortcut: Some("Left"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::MoveDocumentTabCursor(-1)),
        },
        CommandSpec {
            id: "document.tab_next",
            title: "Next Tab In Strip",
            keywords: &["strip", "cursor"],
            shortcut: Some("Right"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::MoveDocumentTabCursor(1)),
        },
        CommandSpec {
            id: "document.activate_tab",
            title: "Activate Document Tab",
            keywords: &["open", "new", "strip"],
            shortcut: Some("Enter"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ActivateDocumentTab),
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
            id: "layout.cycle",
            title: "Cycle Layout",
            keywords: &["preset", "panes", "split"],
            shortcut: Some("F10"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CycleLayout),
        },
        CommandSpec {
            id: "layout.hide_explorer",
            title: "Hide Explorer",
            keywords: &["layout", "pane", "toggle", "sidebar"],
            shortcut: Some("Alt+E"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::HideExplorer),
        },
        CommandSpec {
            id: "layout.hide_results",
            title: "Hide Results",
            keywords: &["layout", "pane", "toggle", "grid"],
            shortcut: Some("Alt+R"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::HideResults),
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
            id: "data.discard_all",
            title: "Discard All Pending Changes",
            keywords: &["revert", "cancel", "rows"],
            shortcut: Some("Ctrl+Shift+R"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::DiscardAllChanges),
        },
        CommandSpec {
            id: "data.toggle_delete",
            title: "Toggle Row Delete",
            keywords: &["remove", "restore", "row"],
            shortcut: Some("Delete"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ToggleRowDelete),
        },
        CommandSpec {
            id: "data.insert_row",
            title: "Insert Row",
            keywords: &["new", "create", "row"],
            shortcut: Some("Ctrl+N"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenInsertRow),
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
            shortcut: Some("Ctrl+S"),
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
            shortcut: Some("e"),
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
            id: "results.cycle_view",
            title: "Cycle Output View",
            keywords: &["grid", "explain", "messages", "output", "results"],
            shortcut: Some("v"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CycleResultsView),
        },
        CommandSpec {
            id: "explain.analyze",
            title: "Explain Analyze",
            keywords: &["analyze", "execute", "timing", "actual"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ConfirmExplainAnalyze),
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
            title: "Connect or Expand",
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
            shortcut: Some("i"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenObjectInspector),
        },
        CommandSpec {
            id: "explorer.ddl",
            title: "Open Object DDL",
            keywords: &["create", "script"],
            shortcut: Some("d"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenObjectDdl),
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
            id: "document.prev_focus",
            title: "Previous Document Tab Focus",
            keywords: &["editor", "tab"],
            shortcut: Some("Alt+Left"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::PrevDocumentTabFocus),
        },
        CommandSpec {
            id: "document.next_focus",
            title: "Next Document Tab Focus",
            keywords: &["editor", "tab"],
            shortcut: Some("Alt+Right"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::NextDocumentTabFocus),
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
            id: "document.rename",
            title: "Rename Document",
            keywords: &["editor", "tab", "title"],
            shortcut: Some("F2"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::RenameDocument),
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
            id: "settings.mode",
            title: "Toggle Light/Dark Mode",
            keywords: &["dark", "light", "contrast"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CycleMode),
        },
        CommandSpec {
            id: "settings.accent",
            title: "Cycle Accent Color",
            keywords: &["accent", "primary", "color"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::CycleAccent),
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
            id: "settings.animation",
            title: "Toggle Animation",
            keywords: &["intro", "splash"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ToggleAnimation),
        },
        CommandSpec {
            id: "settings.unicode",
            title: "Toggle Unicode Glyphs",
            keywords: &["ascii", "glyph"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ToggleUnicode),
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
            id: "connection.browse",
            title: "Browse Connections",
            keywords: &["database", "sessions", "profiles"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenConnections),
        },
        CommandSpec {
            id: "explorer.actions",
            title: "Object Actions",
            keywords: &["menu", "context", "sidebar"],
            shortcut: Some("a"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenNodeMenu),
        },
        CommandSpec {
            id: "connection.test",
            title: "Test Connection",
            keywords: &["ping", "check", "reach"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::TestConnection),
        },
        CommandSpec {
            id: "connection.duplicate",
            title: "Duplicate Connection",
            keywords: &["copy", "clone", "profile"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::DuplicateConnection),
        },
        CommandSpec {
            id: "connection.move_group",
            title: "Move to Group",
            keywords: &["folder", "organise", "organize"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditConnectionGroup),
        },
        CommandSpec {
            id: "connection.delete",
            title: "Delete Connection",
            keywords: &["remove", "drop", "profile"],
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
            keywords: &["theme", "mode", "accent", "keymap", "mouse"],
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
            id: "editor.undo",
            title: "Undo",
            keywords: &["revert", "back"],
            shortcut: Some("Ctrl+Z"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorUndo),
        },
        CommandSpec {
            id: "editor.redo",
            title: "Redo",
            keywords: &["forward", "again"],
            shortcut: Some("Ctrl+Y"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorRedo),
        },
        CommandSpec {
            id: "editor.paste",
            title: "Paste",
            keywords: &["clipboard", "insert"],
            shortcut: Some("Ctrl+V"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::PasteFromClipboard),
        },
        CommandSpec {
            id: "editor.select_all",
            title: "Select All",
            keywords: &["selection", "everything"],
            shortcut: Some("Ctrl+A"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorSelectAll),
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
            shortcut: None,
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

/// Commands that stay invokable by key or by id, but are not worth a row in the
/// palette: cursor motion, pane gestures, tab switching, and steps that are already
/// a labelled row inside the screen they belong to.
fn hidden(id: &str) -> bool {
    matches!(
        id,
        // cursor and selection gestures
        "results.up"
            | "results.down"
            | "results.left"
            | "results.right"
            | "results.pageup"
            | "results.pagedown"
            | "results.top"
            | "results.extend_up"
            | "results.extend_down"
            | "results.toggle_pick"
            | "results.actions"
            | "results.select_row"
            | "results.select_column"
            | "explorer.up"
            | "explorer.down"
            | "explorer.expand"
            | "explorer.actions"
            // pane focus and sizing
            | "focus.explorer"
            | "focus.editor"
            | "focus.results"
            | "focus.tabs"
            | "document.activate_tab"
            | "document.tab_prev"
            | "document.tab_next"
            | "layout.hide_explorer"
            | "layout.hide_results"
            | "layout.results_grow"
            | "layout.results_shrink"
            | "layout.explorer_grow"
            | "layout.explorer_shrink"
            // tab switching
            | "document.next"
            | "document.prev"
            | "document.prev_focus"
            | "document.next_focus"
            | "results.next_tab"
            | "results.cycle_view"
            | "results.prev_tab"
            // already a labelled row inside the Settings screen
            | "settings.mode"
            | "settings.accent"
            | "settings.keymap"
            | "settings.mouse"
            | "settings.animation"
            | "settings.unicode"
            | "settings.reset"
            // second step of a flow the palette already opened
            | "recovery.restore"
            | "recovery.discard"
            // grid chrome
            | "data.page_next"
            | "data.page_prev"
            | "data.toggle_delete"
            | "data.nav_back"
            // listing the palette inside the palette
            | "palette.open"
            // opening the palette destroys completion state, so it is always disabled
            | "editor.accept_completion"
    )
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
    all_entries(model)
        .into_iter()
        .filter(|entry| !hidden(entry.id))
        .collect()
}

/// Every command as a row, hidden ones included. The context menu addresses commands
/// by id and lists some the palette deliberately leaves out, `explorer.expand` first
/// among them.
pub fn all_entries(model: &Model) -> Vec<PaletteEntry> {
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
        Requirement::SelectedConnection => model.connections.selected().is_none(),
        Requirement::LoadedDdl => model.inspector.ddl.is_none(),
        Requirement::PendingChanges => model.data.changes.pending().is_empty(),
        Requirement::ActiveQuery => model.active_operation.is_none(),
        Requirement::Parameters => model.editor.parameters.is_empty(),
        Requirement::History => model.editor.history.is_empty(),
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
        | "data.page_prev"
        | "data.insert_row" => &[ActiveSession],
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
        "data.inspect"
        | "data.related"
        | "results.actions"
        | "results.toggle_pick"
        | "data.toggle_delete" => &[Results, RowSelection],
        "explorer.actions"
        | "explorer.expand"
        | "explorer.copy_name"
        | "explorer.copy_simple"
        | "explorer.favorite"
        | "explorer.up"
        | "explorer.down" => &[ExplorerNode],
        "transfer.import"
        | "backup.dump"
        | "backup.restore"
        | "explorer.refresh_all"
        | "explain.analyze" => &[ActiveSession],
        "explorer.refresh" => &[ActiveSession, ExplorerNode],
        "connection.test"
        | "connection.duplicate"
        | "connection.move_group"
        | "connection.delete" => &[SelectedConnection],
        "explorer.copy_ddl" => &[LoadedDdl],
        "data.revert" | "data.review" | "data.discard_all" => &[PendingChanges],
        "query.cancel" => &[ActiveQuery],
        "editor.parameters" => &[Parameters],
        "editor.history.clear" => &[History],
        _ => &[],
    }
}
