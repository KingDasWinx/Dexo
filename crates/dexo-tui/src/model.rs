use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::{Arc, LazyLock};

use dexo_app::event::TaskId;
use dexo_app::{ExecutionTarget, ScriptPolicy};
use dexo_driver_api::{ColumnMeta, DbValue, QueryId, TransactionState};
use dexo_sql::SqlDocument;

use crate::runtime::{OperationId, OperationKey, SessionId};

use crate::capabilities::TerminalCapabilities;
use crate::keymap::{Chord, Keymap};
use crate::layout::{LayoutMode, LayoutPlan, LayoutPreset, PaneLayout};
use crate::mouse::{HitMap, LastClick, PaneEdge};
use crate::screens::admin::AdminScreen;
use crate::screens::config_transfer::ConfigTransferScreen;
use crate::screens::connection::ConnectionForm;
use crate::screens::connections::ConnectionsScreen;
use crate::screens::data::DataScreen;
use crate::screens::diagnostics::DiagnosticsScreen;
use crate::screens::document_name_prompt::DocumentNamePrompt;
use crate::screens::editor::EditorState;
use crate::screens::explain::ExplainScreen;
use crate::screens::explorer::ExplorerState;
use crate::screens::file_picker::{FilePicker, FilePickerMode};
use crate::screens::mcp_audit::McpAuditScreen;
use crate::screens::mcp_profiles::McpProfilesScreen;
use crate::screens::object_inspector::ObjectInspector;
use crate::screens::projects::ProjectsScreen;
use crate::screens::recovery::RecoveryScreen;
use crate::screens::schema_diff::SchemaDiffScreen;
use crate::screens::schema_editor::SchemaEditor;
use crate::screens::secret_prompt::SecretPrompt;
use crate::screens::security::SecurityScreen;
use crate::screens::settings::SettingsScreen;
use crate::screens::transaction_prompt::TransactionPrompt;
use crate::screens::transfer::TransferScreen;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    Explorer,
    Editor,
    Results,
    /// The strip of document tabs. It owns keys of its own, so it is a pane like the
    /// rest; `document_tab_focus` is the cursor inside it, the way `explorer.selected`
    /// is the cursor inside the tree.
    DocumentTabs,
    /// Only a table document has this pane: the grid takes the editor's slot and the
    /// console takes the grid's.
    Console,
    Palette,
}

impl Model {
    /// The table Enter in the Security panel grants on: the one that is open, nothing
    /// while no table is (the data screen's stand-in is called `tbl`).
    pub fn security_grant_target(&self) -> Option<String> {
        let target = &self.data.target;
        (target.schema().is_some() || target.catalog().is_some() || target.object() != "tbl")
            .then(|| target.display_unquoted())
    }

    /// The pane the stored focus actually points at. Opening a table document moves the
    /// grid into the editor's slot, and closing one takes the console away -- either way
    /// the focus left behind would highlight a pane that is not on screen.
    pub fn effective_focus(&self) -> Focus {
        if self.focus == Focus::DocumentTabs {
            return Focus::DocumentTabs;
        }
        if self.active_document().kind.is_table() {
            match self.focus {
                Focus::Editor => Focus::Results,
                other => other,
            }
        } else {
            match self.focus {
                Focus::Console => Focus::Results,
                other => other,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentTabFocus {
    Document(usize),
    New,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DragKind {
    PaneDivider(PaneEdge),
    EditorSelect { anchor: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DragState {
    pub kind: DragKind,
    pub origin_x: u16,
    pub origin_y: u16,
    pub start_value: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectionStatus {
    pub name: String,
    pub ready: bool,
    pub environment: String,
    pub read_only: bool,
    pub driver: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaletteState {
    pub open: bool,
    pub query: crate::widgets::text_input::TextInput,
    pub selected: usize,
    pub offset: usize,
    pub origin_focus: Option<Focus>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HelpState {
    pub open: bool,
    pub scroll: u16,
    pub query: crate::widgets::text_input::TextInput,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OnboardingState {
    pub open: bool,
    pub logo_frames: Arc<Vec<crate::entrance::LogoFrame>>,
    pub logo_frame: usize,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            open: false,
            logo_frames: Arc::new(vec![crate::entrance::static_logo_frame()]),
            logo_frame: 0,
        }
    }
}

/// Context menu over a sidebar node. The renderer derives where it sits from the
/// sidebar layout, so nothing here can fall out of step with the row it points at.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeMenuState {
    pub open: bool,
    pub selected: usize,
    pub offset: usize,
}

/// An execution waiting for the document's connection to open. Replayed rather than
/// resumed: the action re-reads the buffer, so nothing about it can go stale while the
/// connect is in flight.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingExecute {
    /// Who asked. Checked on drain -- the user may have moved to another tab.
    pub document: String,
    pub action: crate::action::Action,
    /// The `connect_token` in flight, so a connect from somewhere else cannot fire it.
    pub token: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateNotice {
    pub version: String,
    pub command: String,
}

/// What to do with a dirty document on its way out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseChoice {
    Save,
    Discard,
    Cancel,
}

impl CloseChoice {
    pub fn next(self) -> Self {
        match self {
            Self::Save => Self::Discard,
            Self::Discard => Self::Cancel,
            Self::Cancel => Self::Save,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Save => Self::Cancel,
            Self::Discard => Self::Save,
            Self::Cancel => Self::Discard,
        }
    }
}

/// Asked when a document with unsaved changes is closed. Closing used to either save
/// on its own or, for an untitled document, refuse -- there was no way to let the
/// changes go.
#[derive(Clone, Debug, PartialEq)]
pub struct ClosePrompt {
    /// Which document, by id: the answer applies to it even if the active one moves.
    pub document: String,
    pub title: String,
    /// Starts on Save, so Enter never throws work away by reflex.
    pub choice: CloseChoice,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResultsMenuState {
    pub open: bool,
    pub selected: usize,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridViewport {
    pub row_offset: usize,
    pub column_offset: usize,
    pub height: usize,
    pub width: usize,
}

impl Default for GridViewport {
    fn default() -> Self {
        Self {
            row_offset: 0,
            column_offset: 0,
            height: 20,
            width: 80,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisibleRow<'a> {
    pub source_index: usize,
    pub cells: &'a [DbValue],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResultBuffer {
    pub columns: Vec<ColumnMeta>,
    rows: Arc<Vec<Vec<DbValue>>>,
    estimated_bytes: usize,
    truncated: bool,
}

impl ResultBuffer {
    pub const MAX_ROWS: usize = 100_000;
    pub const MAX_BYTES: usize = 32 * 1024 * 1024;

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn rows(&self) -> &[Vec<DbValue>] {
        &self.rows
    }

    pub fn rows_snapshot(&self) -> Arc<Vec<Vec<DbValue>>> {
        Arc::clone(&self.rows)
    }

    pub fn estimated_bytes(&self) -> usize {
        self.estimated_bytes
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn append_rows(&mut self, rows: Vec<Vec<DbValue>>) {
        let storage = Arc::make_mut(&mut self.rows);
        for row in rows {
            let added = estimated_row_bytes(&row);
            if storage.len() >= Self::MAX_ROWS
                || self.estimated_bytes.saturating_add(added) > Self::MAX_BYTES
            {
                self.truncated = true;
                break;
            }
            self.estimated_bytes += added;
            storage.push(row);
        }
    }

    pub fn clear(&mut self) {
        self.columns.clear();
        self.rows = Arc::default();
        self.estimated_bytes = 0;
        self.truncated = false;
    }

    pub fn remove_row(&mut self, index: usize) {
        let storage = Arc::make_mut(&mut self.rows);
        if index >= storage.len() {
            return;
        }
        let removed = storage.remove(index);
        self.estimated_bytes = self
            .estimated_bytes
            .saturating_sub(estimated_row_bytes(&removed));
    }

    pub fn set_cell(&mut self, row: usize, col: usize, value: DbValue) {
        let storage = Arc::make_mut(&mut self.rows);
        if let Some(cell) = storage.get_mut(row).and_then(|cells| cells.get_mut(col)) {
            let old = estimated_row_bytes(std::slice::from_ref(cell));
            let new = estimated_row_bytes(std::slice::from_ref(&value));
            self.estimated_bytes = self.estimated_bytes.saturating_sub(old).saturating_add(new);
            *cell = value;
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GridSelection {
    Cell {
        row: usize,
        col: usize,
    },
    Row {
        row: usize,
    },
    Column {
        col: usize,
    },
    Range {
        start: (usize, usize),
        end: (usize, usize),
    },
}

impl Default for GridSelection {
    fn default() -> Self {
        Self::Cell { row: 0, col: 0 }
    }
}

/// Rows put on the clipboard, with how many rows and columns they are.
pub struct Copied {
    pub text: String,
    pub rows: usize,
    pub columns: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridModel {
    buffer: ResultBuffer,
    viewport: GridViewport,
    selection: Option<(usize, usize)>,
    column_widths: Vec<u16>,
    pub kind: GridSelection,
    pub picked_rows: BTreeSet<usize>,
    pub frozen_columns: usize,
    pub hidden_columns: Vec<usize>,
    pub cells: std::collections::BTreeMap<(usize, usize), GridCell>,
    /// Lines of the record view scrolled off the top, for a record taller than the pane.
    pub record_scroll: usize,
    /// The column the cursor starts in when rows arrive: the one it was in before the grid
    /// was run again, so `s` on a column sorts it again and not the first.
    pub home_column: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OperationStatus {
    #[default]
    Idle,
    Running,
    Finished,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GridCell {
    Inline(DbValue),
    Spool {
        id: uuid::Uuid,
        path: std::path::PathBuf,
        loaded: u64,
        total: u64,
    },
    Remote(dexo_driver_api::RemoteValueRef),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResultKey {
    pub operation: OperationKey,
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResultTab {
    pub key: ResultKey,
    pub title: String,
    pub grid: GridModel,
    pub status: OperationStatus,
    pub rows_affected: Option<u64>,
    pub notices: Vec<String>,
    pub source_sql: Option<String>,
    pub local_only: Option<String>,
    /// Where the statement began in its document, in bytes, and the document's revision
    /// then: what turns the server's position for a failure into a place in the text.
    pub source_offset: Option<(usize, u64)>,
    /// The rows stopped at the row limit with more left.
    pub truncated: bool,
    /// A page of the statement run again with the bars (`LIMIT`/`OFFSET`): its rows are
    /// one page, not the whole result, and n and p turn it.
    pub paged: bool,
}

impl ResultTab {
    pub fn new(key: ResultKey, title: impl Into<String>) -> Self {
        Self {
            key,
            title: title.into(),
            grid: GridModel::default(),
            status: OperationStatus::Idle,
            rows_affected: None,
            notices: Vec::new(),
            source_sql: None,
            local_only: None,
            source_offset: None,
            truncated: false,
            paged: false,
        }
    }
}

/// How a message reads at a glance. Nothing else in the app records these -- the log is
/// never rendered -- so the toast is the only chance the user gets to see one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// It happened.
    Info,
    /// You asked, and a precondition stopped it. Nothing is broken.
    Warn,
    /// It failed.
    Error,
}

impl Severity {
    /// Ticks the toast survives, a tick being a second. An error stays the longest, long
    /// enough to read a sentence; it is in the Messages view after that. One that stayed
    /// until Esc covered the editor for minutes and outlived the action that fixed it.
    pub fn ticks(self) -> u8 {
        match self {
            Self::Info => 4,
            Self::Warn => 6,
            Self::Error => 12,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// One entry in the log. The toast shows the newest; the Messages view shows them all.
#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub message: String,
    pub severity: Severity,
    /// Local wall-clock time, `HH:MM:SS`.
    pub at: String,
    /// Lines under the message: where the server said it failed, SQLSTATE, DETAIL, HINT.
    pub details: Vec<String>,
}

impl Notification {
    /// Rows the entry takes in the Messages view before wrapping.
    pub fn line_count(&self) -> usize {
        1 + self.details.len()
    }
}

/// Local wall-clock time as `HH:MM:SS`, for the console and the Messages view.
pub fn clock() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

/// What the server said about a failed statement, laid out the way psql prints it: the
/// SQLSTATE and position, the offending line with a caret under the failing token, then
/// DETAIL and HINT. `statement` is `(index, count)` so a script names which one failed.
pub fn describe_query_error(
    sql: &str,
    error: &dexo_driver_api::DriverError,
    statement: (usize, usize),
) -> Vec<String> {
    let mut lines = Vec::new();
    let location = error
        .position()
        .and_then(|position| sql_location(sql, position));
    let mut header = Vec::new();
    if let Some(code) = error.native_code() {
        header.push(describe_native_code(code));
    }
    if let Some((line, column, _)) = &location {
        header.push(format!("line {line}, column {column}"));
    }
    if statement.1 > 1 {
        header.push(format!("statement {} of {}", statement.0 + 1, statement.1));
    }
    if !header.is_empty() {
        lines.push(header.join(" · "));
    }
    if let Some((line, column, text)) = location {
        let prefix = format!("LINE {line}: ");
        lines.push(format!("{prefix}{text}"));
        lines.push(format!(
            "{}^",
            " ".repeat(prefix.chars().count() + column - 1)
        ));
    }
    if let Some(detail) = error.detail() {
        lines.push(format!("DETAIL: {detail}"));
    }
    if let Some(hint) = error.hint() {
        lines.push(format!("HINT: {hint}"));
    }
    lines
}

/// The driver's own code for a failure, named for what it is: Postgres gives a SQLSTATE,
/// MySQL its error number with the SQLSTATE after it, SQLite a result code, which has no
/// SQLSTATE, and DuckDB the kind of error.
fn describe_native_code(code: &str) -> String {
    let sqlstate = |text: &str| {
        text.len() == 5
            && text
                .chars()
                .all(|ch| ch.is_ascii_digit() || ch.is_ascii_uppercase())
    };
    if sqlstate(code) {
        return format!("SQLSTATE {code}");
    }
    if let Some((number, state)) = code.split_once(" (")
        && let Some(state) = state.strip_suffix(')')
        && sqlstate(state)
        && number.chars().all(|ch| ch.is_ascii_digit())
    {
        return format!("error {number} · SQLSTATE {state}");
    }
    if code.chars().all(|ch| ch.is_ascii_digit()) {
        return format!("error code {code}");
    }
    code.to_string()
}

/// 1-based line and column of a 1-based character offset, with that line's text (tabs
/// widened to one space so the caret stays under the character it points at).
fn sql_location(sql: &str, position: u32) -> Option<(usize, usize, String)> {
    let offset = (position as usize).checked_sub(1)?;
    let mut line_start = 0;
    let mut line = 1;
    for (index, ch) in sql.chars().enumerate() {
        if index == offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = index + 1;
        }
    }
    if offset > sql.chars().count() {
        return None;
    }
    let text: String = sql
        .chars()
        .skip(line_start)
        .take_while(|ch| *ch != '\n')
        .map(|ch| if ch == '\t' { ' ' } else { ch })
        .collect();
    Some((line, offset - line_start + 1, text.trim_end().to_string()))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub message: String,
    pub severity: Severity,
    pub ticks_left: u8,
}

/// The message log, plus the transient toast that surfaces the newest entry. The log
/// used to be appended to the status bar, which is the one place a user never looks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notifications {
    entries: Vec<Notification>,
    pub toast: Option<Toast>,
}

impl Notifications {
    /// It happened.
    pub fn info(&mut self, message: String) {
        self.emit(Severity::Info, message);
    }

    /// You asked, and a precondition stopped it.
    pub fn warn(&mut self, message: String) {
        self.emit(Severity::Warn, message);
    }

    /// It failed.
    pub fn error(&mut self, message: String) {
        self.emit(Severity::Error, message);
    }

    /// It failed, and the server said more than one line about it.
    pub fn error_with(&mut self, message: String, details: Vec<String>) {
        self.emit_with(Severity::Error, message, details);
    }

    fn emit(&mut self, severity: Severity, message: String) {
        self.emit_with(severity, message, Vec::new());
    }

    fn emit_with(&mut self, severity: Severity, message: String, details: Vec<String>) {
        self.toast = Some(Toast {
            message: message.clone(),
            severity,
            ticks_left: severity.ticks(),
        });
        self.entries.push(Notification {
            message,
            severity,
            at: clock(),
            details,
        });
    }

    /// Rows every entry takes, the bound the Messages view scrolls within.
    pub fn line_count(&self) -> usize {
        self.entries.iter().map(Notification::line_count).sum()
    }

    /// Scroll offset that puts the newest entry at the top of the view.
    pub fn newest_offset(&self) -> usize {
        self.line_count()
            .saturating_sub(self.entries.last().map_or(0, Notification::line_count))
    }

    pub fn last(&self) -> Option<&Notification> {
        self.entries.last()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Notification> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn dismiss(&mut self) {
        self.toast = None;
    }

    /// True while a toast is up that will age out on its own -- the clock runs only then.
    pub fn expires(&self) -> bool {
        self.toast
            .as_ref()
            .is_some_and(|toast| toast.ticks_left > 0)
    }

    /// Ages the visible toast.
    pub fn tick(&mut self) {
        match &mut self.toast {
            Some(toast) if toast.ticks_left > 1 => toast.ticks_left -= 1,
            Some(toast) if toast.ticks_left == 1 => self.toast = None,
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResultsState {
    pub tabs: Vec<ResultTab>,
    pub active: usize,
    /// Which projection of the output pane is on screen. Explain used to be a
    /// workbench tab, which put query output in two unrelated places.
    pub view: ResultsView,
    /// Explain scrolls on its own; it used to share `tabs.scroll` with four tabs
    /// that had nothing to do with it.
    pub explain_scroll: u16,
    pub messages_scroll: u16,
    /// This document's plan. It lived on the model, so every tab showed the last plan
    /// explained anywhere.
    pub explain: ExplainScreen,
    /// The result a sort or clause re-run replaced, until the run answers: kept with the
    /// document's output, so a failure puts it back in that document and no other.
    pub derived_backup: Option<DerivedBackup>,
    /// Last size the output pane handed down. Kept so a tab created between two syncs
    /// is born the right size instead of with `GridViewport`'s defaults.
    viewport_size: (u16, u16),
}

/// Views of the output pane, in selector order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ResultsView {
    #[default]
    Grid,
    Explain,
    /// The log, which until now had no surface at all: a message got one toast and was
    /// then unreachable.
    Messages,
}

impl ResultsView {
    pub const ALL: [Self; 3] = [Self::Grid, Self::Explain, Self::Messages];

    pub fn label(self) -> &'static str {
        match self {
            Self::Grid => "Grid",
            Self::Explain => "Explain",
            Self::Messages => "Messages",
        }
    }
}

impl Default for ResultsState {
    fn default() -> Self {
        let empty = GridViewport::default();
        Self {
            tabs: Vec::new(),
            active: 0,
            view: ResultsView::default(),
            explain_scroll: 0,
            messages_scroll: 0,
            explain: ExplainScreen::default(),
            derived_backup: None,
            viewport_size: (empty.width as u16, empty.height as u16),
        }
    }
}

impl ResultsState {
    /// Every tab is drawn in the same pane, so the pane sizes all of them. Sizing only
    /// the active one left the others believing in `GridViewport::default()`, and
    /// switching to one walked the cursor past the rows the pane paints.
    pub fn set_viewport_size(&mut self, width: u16, height: u16) {
        self.viewport_size = (width, height);
        for tab in &mut self.tabs {
            tab.grid.set_viewport_size(width, height);
        }
    }

    pub fn push_tab(&mut self, mut tab: ResultTab) {
        let (width, height) = self.viewport_size;
        tab.grid.set_viewport_size(width, height);
        self.tabs.push(tab);
    }

    /// Replaces every result set with one, as reloading a table's data does.
    pub fn replace_tabs(&mut self, tab: ResultTab) {
        self.tabs.clear();
        self.active = 0;
        self.push_tab(tab);
    }

    fn grid(&self) -> &GridModel {
        self.tabs
            .get(self.active)
            .map(|tab| &tab.grid)
            .unwrap_or_else(|| &*EMPTY_GRID)
    }

    fn grid_mut(&mut self) -> &mut GridModel {
        if self.tabs.is_empty() {
            self.push_tab(ResultTab::new(
                ResultKey {
                    operation: OperationKey::new(OperationId::new(), "", "", 0),
                    index: 0,
                },
                "result",
            ));
            self.active = 0;
        }
        let index = self.active.min(self.tabs.len() - 1);
        &mut self.tabs[index].grid
    }
}

static EMPTY_GRID: LazyLock<GridModel> = LazyLock::new(GridModel::default);

impl std::ops::Deref for ResultsState {
    type Target = GridModel;

    fn deref(&self) -> &GridModel {
        self.grid()
    }
}

impl std::ops::DerefMut for ResultsState {
    fn deref_mut(&mut self) -> &mut GridModel {
        self.grid_mut()
    }
}

impl GridModel {
    pub fn sample_rows(count: usize) -> Self {
        let mut buffer = ResultBuffer {
            columns: vec![ColumnMeta {
                name: "n".into(),
                type_name: "int8".into(),
                nullable: false,
            }],
            ..ResultBuffer::default()
        };
        buffer.append_rows(
            (0..count)
                .map(|index| vec![DbValue::I64(index as i64)])
                .collect(),
        );
        Self {
            buffer,
            viewport: GridViewport::default(),
            selection: Some((0, 0)),
            column_widths: vec![8],
            kind: GridSelection::Cell { row: 0, col: 0 },
            picked_rows: BTreeSet::new(),
            frozen_columns: 0,
            hidden_columns: Vec::new(),
            cells: std::collections::BTreeMap::new(),
            record_scroll: 0,
            home_column: 0,
        }
    }

    pub fn with_viewport(mut self, row_offset: usize, height: usize) -> Self {
        self.viewport.row_offset = row_offset;
        self.viewport.height = height;
        self
    }

    pub fn visible_rows(&self) -> Vec<VisibleRow<'_>> {
        self.visible_slice(self.viewport.row_offset, self.viewport.height)
    }

    pub fn visible_slice(&self, row_offset: usize, height: usize) -> Vec<VisibleRow<'_>> {
        let start = row_offset.min(self.buffer.row_count());
        let end = (start + height).min(self.buffer.row_count());
        self.buffer.rows()[start..end]
            .iter()
            .enumerate()
            .map(|(index, row)| VisibleRow {
                source_index: start + index,
                cells: row,
            })
            .collect()
    }

    pub fn row_count(&self) -> usize {
        self.buffer.row_count()
    }

    pub fn columns(&self) -> &[ColumnMeta] {
        &self.buffer.columns
    }

    pub fn set_columns(&mut self, columns: Vec<ColumnMeta>) {
        self.buffer.columns = columns;
        self.column_widths.clear();
        self.recompute_column_widths();
        self.ensure_cursor();
    }

    pub fn append_rows(&mut self, rows: Vec<Vec<DbValue>>) {
        for row in rows {
            let row_index = self.buffer.row_count();
            let mut display = Vec::with_capacity(row.len());
            for (col, value) in row.into_iter().enumerate() {
                let (shown, deferred) = bound_value(value);
                if let Some(cell) = deferred {
                    self.cells.insert((row_index, col), cell);
                }
                display.push(shown);
            }
            self.buffer.append_rows(vec![display]);
        }
        self.recompute_column_widths();
        self.ensure_cursor();
    }

    pub fn clear(&mut self) {
        for cell in self.cells.values() {
            if let GridCell::Spool { path, .. } = cell {
                let _ = std::fs::remove_file(path);
            }
        }
        self.cells.clear();
        self.buffer.clear();
        self.viewport.row_offset = 0;
        self.viewport.column_offset = 0;
        self.selection = None;
        // A column or a range picked in the last result is not one in this.
        self.kind = GridSelection::default();
        self.home_column = 0;
        self.record_scroll = 0;
        self.picked_rows.clear();
        self.column_widths.clear();
    }

    pub fn remove_row(&mut self, index: usize) {
        let mut shifted = std::collections::BTreeMap::new();
        for (&(row, col), cell) in self.cells.iter() {
            if row == index {
                if let GridCell::Spool { path, .. } = cell {
                    let _ = std::fs::remove_file(path);
                }
                continue;
            }
            let new_row = if row > index { row - 1 } else { row };
            shifted.insert((new_row, col), cell.clone());
        }
        self.cells = shifted;
        self.buffer.remove_row(index);
        self.picked_rows = self
            .picked_rows
            .iter()
            .filter_map(|&row| match row.cmp(&index) {
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(row - 1),
                std::cmp::Ordering::Less => Some(row),
            })
            .collect();
        self.ensure_cursor();
    }

    /// Shows `value` in a cell: an edit not applied yet, drawn where it will be.
    pub fn set_cell(&mut self, row: usize, col: usize, value: DbValue) {
        self.buffer.set_cell(row, col, value);
        self.recompute_column_widths();
    }

    pub fn cell_at(&self, row: usize, col: usize) -> Option<&GridCell> {
        self.cells.get(&(row, col))
    }

    pub fn viewport(&self) -> GridViewport {
        self.viewport
    }

    pub fn set_viewport_size(&mut self, width: u16, height: u16) {
        self.viewport.width = width as usize;
        self.viewport.height = height as usize;
        self.clamp_scroll();
        if let Some(row) = self.cursor_row() {
            self.ensure_row_visible(row);
        }
        self.recompute_column_widths();
    }

    pub fn scroll_rows(&mut self, delta: i32) {
        let next = self.viewport.row_offset as i32 + delta;
        self.viewport.row_offset = next.max(0) as usize;
        self.clamp_scroll();
        self.recompute_column_widths();
    }

    pub fn scroll_columns(&mut self, delta: i32) {
        let next = self.viewport.column_offset as i32 + delta;
        self.viewport.column_offset = next.max(0) as usize;
        self.clamp_scroll();
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        self.selection
    }

    pub fn select_cell(&mut self, row: usize, col: usize) {
        self.kind = GridSelection::Cell { row, col };
        self.selection = Some((row, col));
    }

    /// The row is selected whole; the column the cursor was on stays, so `s` still sorts
    /// the column it sorted a moment ago.
    pub fn select_row(&mut self, row: usize) {
        let col = self.selection.map_or(0, |(_, col)| col);
        self.kind = GridSelection::Row { row };
        self.selection = Some((row, col));
    }

    pub fn select_column(&mut self, col: usize) {
        self.kind = GridSelection::Column { col };
        self.selection = Some((0, col));
    }

    pub fn select_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        self.kind = GridSelection::Range { start, end };
        self.selection = Some(end);
    }

    pub fn ensure_cursor(&mut self) {
        if self.buffer.row_count() == 0 {
            self.selection = None;
            return;
        }
        if self.selection.is_none() {
            let last = self.buffer.columns.len().saturating_sub(1);
            self.select_cell(0, self.home_column.min(last));
            self.ensure_column_visible(self.home_column.min(last));
        }
    }

    pub fn cursor_row(&self) -> Option<usize> {
        self.selection.map(|(row, _)| row)
    }

    pub fn row_selected(&self, row: usize) -> bool {
        if self.picked_rows.contains(&row) {
            return true;
        }
        match self.kind {
            GridSelection::Cell { row: r, .. } | GridSelection::Row { row: r } => r == row,
            GridSelection::Column { .. } => self.selection.is_some_and(|(r, _)| r == row),
            GridSelection::Range { start, end } => {
                let lo = start.0.min(end.0);
                let hi = start.0.max(end.0);
                row >= lo && row <= hi
            }
        }
    }

    pub fn toggle_picked_row(&mut self) {
        self.ensure_cursor();
        let Some(row) = self.cursor_row() else {
            return;
        };
        if !self.picked_rows.remove(&row) {
            self.picked_rows.insert(row);
        }
    }

    pub fn move_cursor_row(&mut self, delta: i32, extend: bool) {
        self.ensure_cursor();
        let Some((row, col)) = self.selection else {
            return;
        };
        let last = self.buffer.row_count().saturating_sub(1);
        let next = (row as i32 + delta).clamp(0, last as i32) as usize;
        if extend {
            self.picked_rows.clear();
            let start = match self.kind {
                GridSelection::Range { start, .. } => start,
                GridSelection::Cell { row, col } => (row, col),
                GridSelection::Row { row } => (row, col),
                GridSelection::Column { col } => (row, col),
            };
            self.select_range(start, (next, col));
        } else {
            self.select_cell(next, col);
        }
        self.ensure_row_visible(next);
    }

    /// Left and Right: the current column -- the one `s` sorts and the header marks --
    /// moves among the shown columns, the view following it. A row cursor or a range
    /// stays what it is.
    pub fn move_cursor_col(&mut self, delta: i32) {
        self.ensure_cursor();
        let Some((row, col)) = self.selection else {
            self.scroll_columns(delta);
            return;
        };
        let shown: Vec<usize> = (0..self.buffer.columns.len())
            .filter(|index| !self.hidden_columns.contains(index))
            .collect();
        let at = shown.iter().position(|index| *index >= col).unwrap_or(0) as i32;
        let Some(&next) = shown.get((at + delta).clamp(0, shown.len() as i32 - 1) as usize) else {
            return;
        };
        match &mut self.kind {
            GridSelection::Cell { col, .. } | GridSelection::Column { col } => *col = next,
            GridSelection::Row { .. } | GridSelection::Range { .. } => {}
        }
        self.selection = Some((row, next));
        self.ensure_column_visible(next);
    }

    /// Scrolls sideways just enough for column `col` to be on screen, as the widths the
    /// viewport was last fitted with lay the columns out.
    fn ensure_column_visible(&mut self, col: usize) {
        let frozen = self.frozen_columns;
        if col < frozen {
            return;
        }
        if col < self.viewport.column_offset {
            self.viewport.column_offset = col;
            return;
        }
        // The grid lays its columns out with `allocate_column_widths`, so the view moves
        // on until that layout has `col` whole: the same answer the painting will give.
        loop {
            let start = self.viewport.column_offset.max(frozen);
            let shown = self.visible_column_indices();
            let natural: Vec<u16> = shown
                .iter()
                .map(|&index| self.column_widths.get(index).copied().unwrap_or(8))
                .collect();
            let (widths, _) = allocate_column_widths(&natural, self.viewport.width);
            let whole = shown
                .iter()
                .position(|&index| index == col)
                .is_some_and(|at| {
                    widths.get(at).is_some_and(|&width| {
                        usize::from(width) >= usize::from(natural[at]).min(self.viewport.width)
                    })
                });
            if whole || start >= col {
                break;
            }
            self.viewport.column_offset = start + 1;
        }
    }

    fn ensure_row_visible(&mut self, row: usize) {
        let height = self.viewport.height.max(1);
        if row < self.viewport.row_offset {
            self.viewport.row_offset = row;
        } else if row >= self.viewport.row_offset.saturating_add(height) {
            self.viewport.row_offset = row.saturating_add(1).saturating_sub(height);
        }
        self.clamp_scroll();
        self.recompute_column_widths();
    }

    pub fn freeze_columns(&mut self, count: usize) {
        self.frozen_columns = count;
    }

    pub fn hide_column(&mut self, col: usize) {
        if !self.hidden_columns.contains(&col) {
            self.hidden_columns.push(col);
        }
    }

    pub fn visible_column_indices(&self) -> Vec<usize> {
        let n = self.column_widths.len().max(self.buffer.columns.len());
        let frozen = self.frozen_columns.min(n);
        let mut out: Vec<usize> = (0..frozen)
            .filter(|index| !self.hidden_columns.contains(index))
            .collect();
        let start = self.viewport.column_offset.max(frozen);
        out.extend((start..n).filter(|index| !self.hidden_columns.contains(index)));
        out
    }

    pub fn copy(
        &self,
        format: dexo_app::data::CopyFormat,
        dialect: dexo_app::data::SqlDialect,
    ) -> Result<String, String> {
        self.copy_of(format, dialect, None)
            .map(|copied| copied.text)
    }

    /// `copy` for a grid showing `table`'s rows, with what it took: an INSERT names the
    /// table, and the toast says how many rows and columns went to the clipboard.
    pub fn copy_of(
        &self,
        format: dexo_app::data::CopyFormat,
        dialect: dexo_app::data::SqlDialect,
        table: Option<&str>,
    ) -> Result<Copied, String> {
        let (columns, rows) = self.selected_matrix();
        let text = dexo_app::data::copy_selection_of(&columns, &rows, format, dialect, table)?;
        Ok(Copied {
            text,
            rows: rows.len(),
            columns: columns.len(),
        })
    }

    /// What "Copy as ..." takes: the cursor's row, or the rows of a range or of the
    /// picked ones, with every column. A column selected as a column stays one.
    pub fn widen_selection_to_rows(&mut self) {
        if !self.picked_rows.is_empty() {
            return;
        }
        match self.kind {
            GridSelection::Range { start, end } => {
                let last_col = self.buffer.columns.len().saturating_sub(1);
                self.select_range((start.0, 0), (end.0, last_col));
            }
            GridSelection::Cell { row, .. } => self.select_row(row),
            GridSelection::Row { .. } | GridSelection::Column { .. } => {}
        }
    }

    fn selected_matrix(&self) -> (Vec<String>, Vec<Vec<DbValue>>) {
        let cols: Vec<usize> = if !self.picked_rows.is_empty() {
            (0..self.buffer.columns.len())
                .filter(|index| !self.hidden_columns.contains(index))
                .collect()
        } else {
            match self.kind {
                GridSelection::Cell { col, .. } | GridSelection::Column { col } => vec![col],
                GridSelection::Row { .. } => (0..self.buffer.columns.len())
                    .filter(|index| !self.hidden_columns.contains(index))
                    .collect(),
                GridSelection::Range { start, end } => {
                    let lo = start.1.min(end.1);
                    let hi = start.1.max(end.1);
                    (lo..=hi)
                        .filter(|index| !self.hidden_columns.contains(index))
                        .collect()
                }
            }
        };
        let names: Vec<String> = cols
            .iter()
            .map(|&index| {
                self.buffer
                    .columns
                    .get(index)
                    .map(|column| column.name.clone())
                    .unwrap_or_else(|| index.to_string())
            })
            .collect();
        let row_idxs: Vec<usize> = if !self.picked_rows.is_empty() {
            self.picked_rows.iter().copied().collect()
        } else {
            match self.kind {
                GridSelection::Cell { row, .. } | GridSelection::Row { row } => vec![row],
                GridSelection::Column { .. } => (0..self.buffer.row_count()).collect(),
                GridSelection::Range { start, end } => {
                    let lo = start.0.min(end.0);
                    let hi = start.0.max(end.0);
                    (lo..=hi).collect()
                }
            }
        };
        let rows = row_idxs
            .into_iter()
            .filter_map(|row| {
                let cells = self.buffer.rows().get(row)?;
                Some(
                    cols.iter()
                        .map(|&col| cells.get(col).cloned().unwrap_or(DbValue::Null))
                        .collect(),
                )
            })
            .collect();
        (names, rows)
    }

    pub fn column_widths(&self) -> &[u16] {
        &self.column_widths
    }

    pub fn rows(&self) -> &[Vec<DbValue>] {
        self.buffer.rows()
    }

    pub fn rows_snapshot(&self) -> Arc<Vec<Vec<DbValue>>> {
        self.buffer.rows_snapshot()
    }

    pub fn truncated(&self) -> bool {
        self.buffer.is_truncated()
    }

    pub fn estimated_bytes(&self) -> usize {
        self.buffer.estimated_bytes()
    }

    fn clamp_scroll(&mut self) {
        let max_row = self
            .buffer
            .row_count()
            .saturating_sub(self.viewport.height.max(1));
        self.viewport.row_offset = self.viewport.row_offset.min(max_row);
        let max_col = self.column_widths.len().saturating_sub(1);
        self.viewport.column_offset = self.viewport.column_offset.min(max_col);
        if let Some((row, col)) = self.selection {
            let row = row.min(self.buffer.row_count().saturating_sub(1));
            let col = col.min(max_col);
            self.selection = if self.buffer.row_count() == 0 {
                None
            } else {
                Some((row, col))
            };
        }
    }

    fn recompute_column_widths(&mut self) {
        let start = self.viewport.row_offset.min(self.buffer.row_count());
        let end = (start + self.viewport.height.max(20)).min(self.buffer.row_count());
        let cols = self.buffer.columns.len().max(
            self.buffer
                .rows()
                .get(start..end)
                .map(|rows| rows.iter().map(Vec::len).max().unwrap_or(0))
                .unwrap_or(0),
        );
        // Widths only grow while a result set is on screen: a column widened
        // for a longer value further down must not clip it again once that
        // row scrolls back into view, and columns must not jitter sideways
        // on every scroll step.
        let previous = std::mem::take(&mut self.column_widths);
        self.column_widths = (0..cols)
            .map(|index| {
                let header = self
                    .buffer
                    .columns
                    .get(index)
                    .map(|column| {
                        unicode_width::UnicodeWidthStr::width(column.name.as_str()) as u16
                    })
                    .unwrap_or(1);
                let body = self.buffer.rows()[start..end]
                    .iter()
                    .map(|row| {
                        row.get(index)
                            .map(|value| {
                                unicode_width::UnicodeWidthStr::width(
                                    cell_text(value, true).as_str(),
                                ) as u16
                            })
                            .unwrap_or(0)
                    })
                    .max()
                    .unwrap_or(0);
                header
                    .max(body)
                    .clamp(1, 40)
                    .saturating_add(COLUMN_PADDING)
                    .max(previous.get(index).copied().unwrap_or(0))
            })
            .collect();
        self.clamp_scroll();
    }
}

pub fn format_value(value: &DbValue) -> String {
    match value {
        DbValue::Null => "NULL".into(),
        DbValue::Bool(v) => v.to_string(),
        DbValue::I64(v) => v.to_string(),
        DbValue::U64(v) => v.to_string(),
        DbValue::Decimal(v) | DbValue::Text(v) | DbValue::Json(v) => v.clone(),
        DbValue::Bytes(_) => "<bytes>".into(),
        DbValue::Native { text, .. } => text.clone(),
    }
}

/// What a grid cell draws for `value`: NULL and the empty string marked, a line break
/// as `↵` (a space without Unicode) and any other control character as a space, one
/// column each. A character that takes no column moved every cell after it one to the
/// left, and widths counted from this are the widths that get drawn.
pub fn cell_text(value: &DbValue, unicode: bool) -> String {
    let text = match value {
        DbValue::Null => "NULL".to_string(),
        DbValue::Text(text) if text.is_empty() => "\"\"".to_string(),
        other => format_value(other),
    };
    let newline = if unicode { '↵' } else { ' ' };
    text.chars()
        .map(|ch| match ch {
            '\n' | '\r' => newline,
            ch if ch.is_control() => ' ',
            ch => ch,
        })
        .collect()
}

pub fn wrap_display_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + char_width > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(ch);
        used += char_width;
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// `text` wrapped to `width` columns at the spaces, so a word is not split across two
/// lines; a word longer than the line is split, as `wrap_display_text` does.
pub fn wrap_words(text: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthStr;
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split(' ') {
        let used = current.width();
        if !current.is_empty() && used + 1 + word.width() <= width {
            current.push(' ');
            current.push_str(word);
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if word.width() <= width {
            current = word.to_string();
        } else {
            let mut pieces = wrap_display_text(word, width);
            current = pieces.pop().unwrap_or_default();
            lines.extend(pieces);
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

pub fn append_field_detail(lines: &mut Vec<String>, name: &str, value: &str, width: usize) {
    if width == 0 {
        lines.push(format!("{name}: {value}"));
        return;
    }
    let label = format!("{name}: ");
    let label_width = unicode_width::UnicodeWidthStr::width(label.as_str());
    if label_width >= width {
        lines.extend(wrap_display_text(&format!("{label}{value}"), width));
        return;
    }
    let value_width = width.saturating_sub(label_width);
    let indent = " ".repeat(label_width);
    let mut first = true;
    for segment in value.split('\n') {
        let wrapped = wrap_display_text(segment, if first { value_width } else { width });
        for (index, line) in wrapped.into_iter().enumerate() {
            if first && index == 0 {
                lines.push(format!("{label}{line}"));
                first = false;
            } else {
                lines.push(format!("{indent}{line}"));
            }
        }
    }
    if first {
        lines.push(label);
    }
}

/// `text` cut to `width` display columns and padded to exactly that many: `format!`'s
/// width counts characters, so a wide one shifted everything after it.
pub fn fit_cell(text: &str, width: usize) -> String {
    let cut = truncate_cell(text, width);
    let used = unicode_width::UnicodeWidthStr::width(cut.as_str());
    format!("{cut}{}", " ".repeat(width.saturating_sub(used)))
}

pub fn truncate_cell(text: &str, width: usize) -> String {
    let text_width = unicode_width::UnicodeWidthStr::width(text);
    if text_width <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + char_width + 1 > width {
            break;
        }
        out.push(ch);
        used += char_width;
    }
    out.push('…');
    out
}

/// `text` in rows of at most `width` columns, broken at a space where there is one and
/// otherwise inside a word. A continuation row is indented under the first, two columns
/// further in. For text a dialog must show whole: cut at its border, the end of a long
/// DDL line was out of reach.
pub fn wrap_line(text: &str, width: usize) -> Vec<String> {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if width < 8 || text.width() <= width {
        return vec![text.to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    let indent = chars
        .iter()
        .take_while(|ch| **ch == ' ')
        .count()
        .min(width / 2);
    let continuation = " ".repeat(indent + 2);
    let column = |ch: &char| ch.width().unwrap_or(0);
    let mut rows = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let prefix = if rows.is_empty() { "" } else { &continuation };
        let room = width - prefix.len();
        let (mut end, mut used) = (start, 0);
        while end < chars.len() && used + column(&chars[end]) <= room {
            used += column(&chars[end]);
            end += 1;
        }
        if end < chars.len() {
            if let Some(space) = (start + 1..end).rev().find(|at| chars[*at] == ' ') {
                end = space + 1;
            } else if end == start {
                end = start + 1;
            }
        }
        let row: String = chars[start..end].iter().collect();
        rows.push(format!("{prefix}{}", row.trim_end()));
        start = end;
        while start < chars.len() && chars[start] == ' ' {
            start += 1;
        }
    }
    rows
}

/// Breathing room past the widest value in a column. Sized to the content alone, the
/// columns sat one space apart with the rest of the pane empty next to them. It is part
/// of the natural width, so `allocate_column_widths` gives it up like any other
/// character when the row stops fitting.
pub const COLUMN_PADDING: u16 = 2;

/// How far a wide column is squeezed so that the columns after it still fit whole.
const SQUEEZE_FLOOR: u16 = 12;

/// The least a column cut by the pane's edge is worth drawing.
const EDGE_MIN: usize = 4;

/// Fits `natural` column widths into `available` terminal columns.
///
/// Narrow columns keep their natural width for as long as possible: when the row
/// doesn't fit, the widest column is shrunk one character at a time -- down to
/// `SQUEEZE_FLOOR`, no further -- instead of greedily truncating whichever column
/// happens to exhaust the remaining space first. When even that does not fit, the
/// columns keep their natural widths and the pane shows as many as it can, from the
/// left: the rest are reached by scrolling sideways, and the second return value is
/// `true`, signaling callers to render an overflow marker.
pub fn allocate_column_widths(natural: &[u16], available: usize) -> (Vec<u16>, bool) {
    let separators = natural.len().saturating_sub(1);
    let floors: Vec<u16> = natural
        .iter()
        .map(|&width| width.min(SQUEEZE_FLOOR))
        .collect();
    let floor_total: usize = floors.iter().map(|&w| usize::from(w)).sum();
    if floor_total + separators <= available {
        return (squeeze(natural, &floors, available - separators), false);
    }
    // Too wide for the pane. Two columns stay free for the marker and its gap.
    let budget = available.saturating_sub(2);
    let mut widths: Vec<u16> = Vec::new();
    let mut used = 0usize;
    for &width in natural {
        let gap = usize::from(!widths.is_empty());
        let room = budget.saturating_sub(used + gap);
        if usize::from(width) <= room {
            widths.push(width);
            used += gap + usize::from(width);
            continue;
        }
        // The column the edge cuts: drawn cut when there is room to read some of it, and
        // always when it is the first, so a pane narrower than one column still shows one.
        if room >= EDGE_MIN || widths.is_empty() {
            widths.push(room.max(1) as u16);
        }
        break;
    }
    (widths, true)
}

/// `natural` widths shrunk, widest first, until they total `budget`; none goes below
/// its `floor`.
fn squeeze(natural: &[u16], floors: &[u16], budget: usize) -> Vec<u16> {
    let mut widths = natural.to_vec();
    let mut total: usize = widths.iter().map(|&w| usize::from(w)).sum();
    let mut heap: std::collections::BinaryHeap<(u16, std::cmp::Reverse<usize>)> = widths
        .iter()
        .enumerate()
        .map(|(index, &width)| (width, std::cmp::Reverse(index)))
        .collect();
    while total > budget {
        let Some((width, std::cmp::Reverse(index))) = heap.pop() else {
            break;
        };
        // The widest is at its floor: so is everything narrower.
        if width <= floors[index].max(1) {
            break;
        }
        widths[index] = width - 1;
        total -= 1;
        heap.push((width - 1, std::cmp::Reverse(index)));
    }
    widths
}

fn estimated_row_bytes(row: &[DbValue]) -> usize {
    row.iter()
        .map(|value| match value {
            DbValue::Null => 0,
            DbValue::Bool(_) => 1,
            DbValue::I64(_) | DbValue::U64(_) => 8,
            DbValue::Decimal(text) | DbValue::Text(text) | DbValue::Json(text) => text.len(),
            DbValue::Bytes(bytes) => bytes.len(),
            DbValue::Native { bytes, text, .. } => bytes.len() + text.len(),
        })
        .sum()
}

fn spool_dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("dexo-spool-{}", std::process::id()))
}

fn bound_value(value: DbValue) -> (DbValue, Option<GridCell>) {
    let inline = dexo_app::data::value::INLINE_BYTES as usize;
    match value {
        DbValue::Bytes(bytes) if bytes.len() > inline => spool_or_prefix(bytes, inline),
        DbValue::Text(text) if text.len() > inline => spool_or_prefix(text.into_bytes(), inline),
        DbValue::Json(text) if text.len() > inline => spool_or_prefix(text.into_bytes(), inline),
        DbValue::Native {
            bytes,
            text,
            type_name,
        } if bytes.len() > inline => {
            let (shown, cell) = spool_or_prefix(bytes, inline);
            match shown {
                DbValue::Bytes(prefix) => (
                    DbValue::Native {
                        type_name,
                        bytes: prefix,
                        text,
                    },
                    cell,
                ),
                other => (other, cell),
            }
        }
        other => (other, None),
    }
}

fn spool_or_prefix(bytes: Vec<u8>, inline: usize) -> (DbValue, Option<GridCell>) {
    let total = bytes.len() as u64;
    let prefix = bytes[..inline.min(bytes.len())].to_vec();
    match crate::runtime::result_spool::spool_bytes(&spool_dir(), &bytes) {
        Ok(file) => (
            DbValue::Bytes(prefix),
            Some(GridCell::Spool {
                id: file.id,
                path: file.path,
                loaded: inline as u64,
                total,
            }),
        ),
        Err(_) => (DbValue::Bytes(prefix), None),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DocumentKind {
    Console,
    Table(dexo_driver_api::QualifiedName),
    /// Nothing is open. The workbench still needs an active document -- eighty call
    /// sites index into the list -- so this stands in for none: no tab, no title, never
    /// persisted, and it becomes a real document the moment it is typed into.
    Placeholder,
}

impl DocumentKind {
    pub fn is_table(&self) -> bool {
        matches!(self, DocumentKind::Table(_))
    }

    pub fn is_placeholder(&self) -> bool {
        matches!(self, DocumentKind::Placeholder)
    }

    /// How the kind is written on a stored document. An editor tab stores nothing;
    /// a table browser stores the table it browses, or it comes back as an editor tab
    /// and the next open of that table makes a second document instead of finding it.
    pub fn storage_tag(&self) -> Option<String> {
        match self {
            Self::Console | Self::Placeholder => None,
            Self::Table(target) => Some(format!("table:{}", target.display_unquoted())),
        }
    }

    pub fn from_storage_tag(tag: &str) -> Option<Self> {
        tag.strip_prefix("table:")
            .map(|target| Self::Table(dexo_app::parse_qualified(target)))
    }
}

#[derive(Clone, Debug)]
pub struct EditorDocument {
    pub id: String,
    pub title: String,
    pub path: Option<std::path::PathBuf>,
    pub connection_id: Option<String>,
    pub sql: SqlDocument,
    pub saved_revision: u64,
    pub viewport_line: usize,
    pub viewport_column: usize,
    pub typing: bool,
    pub anchor: Option<usize>,
    pub kind: DocumentKind,
    pub console_log: Vec<String>,
    /// The document a related row was followed from: this one is that hop's own, and
    /// `b` closes it and goes back there.
    pub related_from: Option<String>,
    /// The output pane as this document last left it. Parked here while another
    /// document is active, so a query run in one file cannot redraw another's grid.
    pub results: ResultsState,
    /// A table's paging, filter, sort and pending edits, parked the same way. Only the
    /// fields `DataScreen::swap_browse` moves mean anything here.
    pub browse: crate::screens::data::DataScreen,
}

impl PartialEq for EditorDocument {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.title == other.title
            && self.path == other.path
            && self.connection_id == other.connection_id
            && self.sql.text() == other.sql.text()
            && self.sql.cursor() == other.sql.cursor()
            && self.saved_revision == other.saved_revision
            && self.viewport_line == other.viewport_line
            && self.viewport_column == other.viewport_column
            && self.typing == other.typing
            && self.anchor == other.anchor
            && self.kind == other.kind
            && self.console_log == other.console_log
    }
}

impl EditorDocument {
    pub fn scratch() -> Self {
        Self {
            id: "scratch".into(),
            title: "scratch.sql".into(),
            path: None,
            connection_id: None,
            sql: SqlDocument::new(""),
            saved_revision: 0,
            viewport_line: 0,
            viewport_column: 0,
            typing: false,
            anchor: None,
            kind: DocumentKind::Console,
            results: ResultsState::default(),
            browse: crate::screens::data::DataScreen::default(),
            console_log: Vec::new(),
            related_from: None,
        }
    }

    /// The stand-in for "nothing open". It used to be a real `scratch.sql`, which
    /// belonged to no connection -- with three connected, there was no saying which
    /// database it was for.
    pub fn placeholder() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            kind: DocumentKind::Placeholder,
            ..Self::scratch()
        }
    }

    pub fn new_unique(
        title: impl Into<String>,
        path: Option<std::path::PathBuf>,
        connection_id: Option<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.into(),
            path,
            connection_id,
            sql: SqlDocument::new(""),
            saved_revision: 0,
            viewport_line: 0,
            viewport_column: 0,
            typing: false,
            anchor: None,
            kind: DocumentKind::Console,
            results: ResultsState::default(),
            browse: crate::screens::data::DataScreen::default(),
            console_log: Vec::new(),
            related_from: None,
        }
    }

    pub fn with_text(text: impl AsRef<str>) -> Self {
        let sql = SqlDocument::new(text.as_ref());
        let saved_revision = sql.revision();
        Self {
            sql,
            saved_revision,
            ..Self::scratch()
        }
    }

    pub fn new_table(
        target: dexo_driver_api::QualifiedName,
        connection_id: Option<String>,
    ) -> Self {
        let title = target.object().to_string();
        let sql_text = format!("SELECT * FROM {} LIMIT 501", target.display_unquoted());
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            path: None,
            connection_id,
            sql: SqlDocument::new(&sql_text),
            saved_revision: 0,
            viewport_line: 0,
            viewport_column: 0,
            typing: false,
            anchor: None,
            kind: DocumentKind::Table(target),
            results: ResultsState::default(),
            browse: crate::screens::data::DataScreen::default(),
            console_log: Vec::new(),
            related_from: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        if self.kind.is_table() {
            return false;
        }
        self.sql.revision() != self.saved_revision
    }

    pub fn text(&self) -> String {
        self.sql.text()
    }

    pub fn cursor(&self) -> usize {
        self.sql.cursor()
    }

    /// The cursor as a byte offset. `SqlDocument` counts the cursor in chars, while
    /// everything in `dexo-sql` that scans the text indexes bytes; handing one the
    /// other's number silently reads the wrong span as soon as the buffer holds a
    /// non-ASCII character.
    pub fn byte_cursor(&self) -> usize {
        let text = self.text();
        text.char_indices()
            .nth(self.cursor())
            .map(|(at, _)| at)
            .unwrap_or(text.len())
    }

    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        let cursor = self.sql.cursor();
        if anchor == cursor {
            None
        } else {
            Some(anchor.min(cursor)..anchor.max(cursor))
        }
    }
}

/// A run that changes the schema, and the table each of its statements creates.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaRun {
    pub operation: crate::runtime::OperationId,
    pub created: Vec<Option<String>>,
    /// The tables each statement drops.
    pub dropped: Vec<Vec<String>>,
}

/// What each statement of a running script does to the transaction -- `BEGIN` opens one,
/// `COMMIT` and `ROLLBACK` close it -- applied as the script reports back. The session
/// only knew of the transactions Dexo's own commands began.
#[derive(Clone, Debug, PartialEq)]
pub struct SqlTransactions {
    pub operation: crate::runtime::OperationId,
    pub session: Option<crate::runtime::SessionId>,
    pub steps: Vec<Option<dexo_driver_api::TransactionState>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DerivedBackup {
    pub operation: crate::runtime::OperationId,
    pub tabs: Vec<ResultTab>,
    pub active: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalEdit {
    pub document: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingDocumentClose {
    pub document: String,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub focus: Focus,
    pub width: u16,
    pub height: u16,
    pub layout_mode: LayoutMode,
    pub connection: ConnectionStatus,
    /// What each open session's driver says it cannot do, and why; the palette disables
    /// those commands for the active one.
    pub unavailable: std::collections::HashMap<
        crate::runtime::SessionId,
        Vec<(dexo_driver_api::Capability, String)>,
    >,
    pub transaction: TransactionState,
    /// The output pane on screen. It belongs to `results_owner`; every other document
    /// keeps its own in `EditorDocument::results` until it is activated again.
    pub results: ResultsState,
    /// Document id whose results `self.results` currently holds. `None` before the
    /// first swap: the pane cannot have been written for anyone but the active
    /// document, so it is adopted rather than dropped. A `Some` id that no longer
    /// matches a document means that document was closed, and its pane goes with it.
    pub results_owner: Option<String>,
    pub palette: PaletteState,
    pub help: HelpState,
    pub onboarding: OnboardingState,
    pub results_menu: ResultsMenuState,
    pub node_menu: NodeMenuState,
    pub pending_execute: Option<PendingExecute>,
    /// A sidebar menu command waiting for the connection it was picked on to connect,
    /// with the `connect_token` in flight.
    pub pending_menu: Option<(u64, crate::palette::PaletteInvocation)>,
    pub close_prompt: Option<ClosePrompt>,
    /// Asked before EXPLAIN ANALYZE runs the statement under the cursor; the focused
    /// footer button while it is open.
    pub explain_prompt: Option<crate::widgets::form::FooterFocus>,
    /// Asked before quitting drops an open transaction or grid edits not yet applied;
    /// the focused footer button while it is open.
    pub quit_prompt: Option<crate::widgets::form::FooterFocus>,
    /// Asked before the editor runs a write on production or a destructive statement.
    pub run_prompt: Option<crate::screens::run_prompt::RunPrompt>,
    /// Asked before any other write reaches production: grid edits, DDL, import,
    /// restore, EXPLAIN ANALYZE of a write.
    pub production_prompt: Option<crate::screens::production_prompt::ProductionPrompt>,
    /// Set for exactly one dispatch once the connection's name was typed.
    pub production_cleared: bool,
    pub layout_preset: LayoutPreset,
    pub messages: Notifications,
    pub documents: Vec<EditorDocument>,
    pub active_document: usize,
    pub document_tab_focus: DocumentTabFocus,
    pub document_tabs_scroll: usize,
    /// Document revision whose tab closes once its matching save is
    /// acknowledged. The buffer is the only copy of the edits until the write
    /// lands.
    pub pending_document_close: Option<PendingDocumentClose>,
    pub execution_target: ExecutionTarget,
    pub script_policy: ScriptPolicy,
    pub active_task: Option<TaskId>,
    pub active_query: Option<QueryId>,
    pub active_operation: Option<OperationId>,
    /// When the running operation started, for the status bar's "running 12s".
    pub active_started: Option<(OperationId, std::time::Instant)>,
    pub active_session: Option<SessionId>,
    pub session_generation: u64,
    pub connect_token: u64,
    pub project: String,
    pub project_id: String,
    pub schema: String,
    pub explorer: ExplorerState,
    /// Catalog objects exactly as the driver produced them. The explorer tree is a view:
    /// it wraps objects in synthetic folders and keeps only what it draws, so a column's
    /// type and a constraint's foreign key do not survive the trip into it. Completion
    /// needs both, so they are kept here as well.
    pub catalog_objects: Vec<dexo_driver_api::CatalogObject>,
    pub catalog_revision: u64,
    /// The connection `catalog_objects` came from. Switching connections used to leave
    /// the previous database's tables and columns on offer.
    pub catalog_connection: String,
    pub inspector: ObjectInspector,
    pub data: DataScreen,
    pub schema_editor: SchemaEditor,
    pub schema_diff: SchemaDiffScreen,
    pub transfer: TransferScreen,
    pub security: SecurityScreen,
    pub admin: AdminScreen,
    pub mcp_profiles: McpProfilesScreen,
    pub connection_form: ConnectionForm,
    pub connections: ConnectionsScreen,
    pub projects: ProjectsScreen,
    pub config_transfer: ConfigTransferScreen,
    pub secret_prompt: SecretPrompt,
    pub transaction_prompt: TransactionPrompt,
    pub document_name_prompt: DocumentNamePrompt,
    pub save_query_prompt: Option<crate::screens::saved_queries::SaveQueryPrompt>,
    /// The Explain view's Try index dialog, while it is open.
    pub try_index: Option<crate::screens::explain::TryIndexPrompt>,
    pub saved_queries: crate::screens::saved_queries::SavedQueriesPicker,
    pub settings: SettingsScreen,
    pub recovery: RecoveryScreen,
    pub mcp_audit: McpAuditScreen,
    pub editor: EditorState,
    /// The completion catalog holds the whole database, so a table it does not list
    /// is one the database does not have.
    pub catalog_complete: bool,
    /// The running script changes the schema: once it ends, the catalog is read again.
    pub schema_run: Option<SchemaRun>,
    pub sql_transactions: Option<SqlTransactions>,
    /// Tables this session's runs created, temporary ones included, which no catalog
    /// lists; with the session and its generation they belong to -- every session's
    /// generation starts at 1, so the generation alone let them leak to another.
    pub session_tables: (
        Option<crate::runtime::SessionId>,
        u64,
        std::collections::HashSet<String>,
    ),
    /// Vim mode's state, when the keymap profile is `vim`.
    pub vim: crate::screens::vim::VimState,
    /// Said once the startup connection is ready, where "Connected" would cover it.
    /// With the name of the connection it is about: only its connect says it.
    pub startup_warning: Option<(String, String)>,
    /// `\x`: the grid shows each row as a record, one field per line, as psql's
    /// expanded display does. For the session, not one result.
    pub expanded_records: bool,
    /// A document waiting to be opened in `$VISUAL` or `$EDITOR`. The event loop owns
    /// the terminal, so it takes this between frames.
    pub external_edit: Option<ExternalEdit>,
    /// Keys arrive through the kitty keyboard protocol. Without it Ctrl+Backspace is sent
    /// as ^H, the same byte as Ctrl+H.
    pub keys_disambiguated: bool,
    /// The find bar at the foot of the editor, Ctrl+F and Ctrl+H.
    pub find: crate::screens::find::FindState,
    pub theme: Theme,
    pub capabilities: TerminalCapabilities,
    /// The user's theme files, `<data dir>/themes/*.toml`, read at start.
    pub user_themes: Vec<crate::theme::UserTheme>,
    /// The theme files' problems already said, so reopening Settings says only new ones.
    pub theme_problems: Vec<String>,
    pub keymap: Keymap,
    pub pending_chord: Chord,
    pub panes: PaneLayout,
    pub mouse: bool,
    pub last_click: Option<LastClick>,
    pub drag: Option<DragState>,
    pub animation: bool,
    /// A newer release and the command that installs it. Kept on the status bar until
    /// the user updates, since a toast is gone before anyone reads the command.
    pub update_notice: Option<UpdateNotice>,
    pub layout_dirty: bool,
    pub hits: HitMap,
    pub file_picker: FilePicker,
    pub file_picker_mode: FilePickerMode,
    pub recent_sql_files: Vec<std::path::PathBuf>,
    pub diagnostic_preview: Option<String>,
    pub diagnostics: DiagnosticsScreen,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            focus: Focus::Editor,
            width: 160,
            height: 50,
            layout_mode: LayoutMode::Full,
            connection: ConnectionStatus {
                name: String::new(),
                ready: false,
                environment: String::new(),
                read_only: false,
                driver: String::new(),
            },
            unavailable: std::collections::HashMap::new(),
            theme: crate::theme::builtin_dark(),
            capabilities: TerminalCapabilities {
                color_depth: crate::capabilities::ColorDepth::TrueColor,
                unicode: true,
                mouse: true,
            },
            keymap: Keymap::default_profile(),
            user_themes: Vec::new(),
            theme_problems: Vec::new(),
            pending_chord: Chord { keys: Vec::new() },
            help: HelpState::default(),
            onboarding: OnboardingState::default(),
            results_menu: ResultsMenuState::default(),
            node_menu: NodeMenuState::default(),
            pending_execute: None,
            pending_menu: None,
            close_prompt: None,
            explain_prompt: None,
            quit_prompt: None,
            run_prompt: None,
            production_prompt: None,
            production_cleared: false,
            layout_preset: LayoutPreset::Normal,
            panes: PaneLayout {
                explorer_visible: true,
                results_visible: true,
                explorer_width: 28,
                results_height: 12,
                console_height: 4,
            },
            mouse: true,
            last_click: None,
            drag: None,
            animation: true,
            update_notice: None,
            layout_dirty: false,
            hits: HitMap::default(),
            file_picker: FilePicker::default(),
            file_picker_mode: FilePickerMode::Open,
            recent_sql_files: Vec::new(),
            diagnostic_preview: None,
            diagnostics: DiagnosticsScreen::default(),
            transaction: TransactionState::Idle,
            results: ResultsState::default(),
            results_owner: None,
            palette: PaletteState::default(),
            messages: Notifications::default(),
            documents: vec![EditorDocument::scratch()],
            active_document: 0,
            document_tab_focus: DocumentTabFocus::Document(0),
            document_tabs_scroll: 0,
            pending_document_close: None,
            execution_target: ExecutionTarget::Document,
            script_policy: ScriptPolicy::StopOnError,
            active_task: None,
            active_query: None,
            active_operation: None,
            active_started: None,
            active_session: None,
            session_generation: 0,
            connect_token: 0,
            project: "default".into(),
            project_id: String::new(),
            schema: String::new(),
            explorer: ExplorerState::default(),
            catalog_objects: Vec::new(),
            catalog_revision: 0,
            catalog_connection: String::new(),
            inspector: ObjectInspector::default(),
            data: DataScreen::default(),
            schema_editor: SchemaEditor::default(),
            schema_diff: SchemaDiffScreen::default(),
            transfer: TransferScreen::default(),
            security: SecurityScreen::default(),
            admin: AdminScreen::default(),
            mcp_profiles: McpProfilesScreen::default(),
            connection_form: ConnectionForm::default(),
            connections: ConnectionsScreen::default(),
            projects: ProjectsScreen::default(),
            config_transfer: ConfigTransferScreen::default(),
            secret_prompt: SecretPrompt::default(),
            transaction_prompt: TransactionPrompt::default(),
            document_name_prompt: DocumentNamePrompt::default(),
            save_query_prompt: None,
            try_index: None,
            saved_queries: Default::default(),
            settings: SettingsScreen::default(),
            recovery: RecoveryScreen::default(),
            mcp_audit: McpAuditScreen::default(),
            editor: EditorState::default(),
            catalog_complete: false,
            schema_run: None,
            sql_transactions: None,
            session_tables: Default::default(),
            vim: crate::screens::vim::VimState::default(),
            startup_warning: None,
            expanded_records: false,
            external_edit: None,
            keys_disambiguated: false,
            find: crate::screens::find::FindState::default(),
        }
    }
}

impl From<Focus> for Model {
    fn from(focus: Focus) -> Self {
        Self {
            focus,
            ..Self::default()
        }
    }
}

impl From<TransactionState> for Model {
    fn from(transaction: TransactionState) -> Self {
        Self {
            transaction,
            ..Self::default()
        }
    }
}

impl Model {
    pub fn fixture(seed: impl Into<Self>) -> Self {
        seed.into()
    }

    /// How long the running statement has been running, while one is.
    pub fn running_for(&self) -> Option<std::time::Duration> {
        let (operation, started) = self.active_started?;
        (self.active_operation == Some(operation)).then(|| started.elapsed())
    }

    /// Why the active session's driver cannot do `capability`, if it cannot.
    pub fn unavailable_reason(&self, capability: dexo_driver_api::Capability) -> Option<&str> {
        self.active_session
            .and_then(|session| self.unavailable.get(&session))?
            .iter()
            .find(|(lacking, _)| *lacking == capability)
            .map(|(_, reason)| reason.as_str())
    }

    /// No real document is open -- the list is empty or holds only the stand-in. The
    /// strip has nothing to put a cursor on but its `+` then.
    pub fn nothing_open(&self) -> bool {
        self.documents
            .iter()
            .all(|document| document.kind.is_placeholder())
    }

    pub fn sync_document_tab_focus(&mut self) {
        if self.nothing_open() {
            self.document_tab_focus = DocumentTabFocus::New;
            return;
        }
        match self.document_tab_focus {
            DocumentTabFocus::Document(index) if index >= self.documents.len() => {
                self.document_tab_focus =
                    DocumentTabFocus::Document(self.active_document.min(self.documents.len() - 1));
            }
            _ => {}
        }
    }

    pub fn advance_document_tab_focus(&mut self, delta: i32) {
        if self.nothing_open() {
            self.document_tab_focus = DocumentTabFocus::New;
            return;
        }
        // Documents only. `+` used to be on this cycle while the focus stayed in the
        // editor, so landing on it left a slot that looked selected and answered no
        // key -- Enter went to the buffer instead. Creating is Ctrl+N from anywhere,
        // and the strip itself still walks onto `+` once it has the focus.
        let current = match self.document_tab_focus {
            DocumentTabFocus::Document(index) => index,
            DocumentTabFocus::New => 0,
        };
        let next = (current as i32)
            .wrapping_add(delta)
            .rem_euclid(self.documents.len() as i32) as usize;
        self.document_tab_focus = DocumentTabFocus::Document(next);
        self.active_document = next;
        self.focus = Focus::Editor;
        self.sync_document_tabs_scroll();
    }

    /// Moves the cursor inside the strip, `+` included, and keeps the focus there. This
    /// is the strip navigating itself; `advance_document_tab_focus` is the shortcut for
    /// changing document without leaving the buffer.
    pub fn move_tab_cursor(&mut self, delta: i32) {
        if self.nothing_open() {
            self.document_tab_focus = DocumentTabFocus::New;
            self.focus = Focus::DocumentTabs;
            return;
        }
        let slots = self.documents.len() + 1;
        let current = match self.document_tab_focus {
            DocumentTabFocus::Document(index) => index,
            DocumentTabFocus::New => self.documents.len(),
        };
        let next = (current as i32)
            .wrapping_add(delta)
            .rem_euclid(slots as i32) as usize;
        self.focus = Focus::DocumentTabs;
        if next < self.documents.len() {
            self.document_tab_focus = DocumentTabFocus::Document(next);
            self.active_document = next;
        } else {
            self.document_tab_focus = DocumentTabFocus::New;
        }
        self.sync_document_tabs_scroll();
    }

    /// The strip's cursor is the active document, except while the strip holds the keys
    /// with the cursor on `+`. The strip lights the cursor's tab, and every change of
    /// document outside it -- a table opened from the tree, the tabs a connect restores,
    /// a close -- moved the active document and left the cursor behind.
    pub fn follow_active_document_tab(&mut self) {
        let wanted = if self.nothing_open() {
            DocumentTabFocus::New
        } else {
            DocumentTabFocus::Document(self.active_document)
        };
        let on_new_in_strip =
            self.focus == Focus::DocumentTabs && self.document_tab_focus == DocumentTabFocus::New;
        if self.document_tab_focus != wanted && !on_new_in_strip {
            self.focus_active_document_tab();
        }
    }

    pub fn focus_active_document_tab(&mut self) {
        if self.nothing_open() {
            self.document_tab_focus = DocumentTabFocus::New;
        } else {
            self.document_tab_focus = DocumentTabFocus::Document(self.active_document);
        }
        self.sync_document_tabs_scroll();
    }

    pub fn document_tabs_area_width(&self) -> u16 {
        crate::layout::LayoutPlan::for_area_with_document_tabs(
            ratatui::layout::Rect::new(0, 0, self.width, self.height),
            Some(&self.effective_panes()),
            true,
        )
        .document_tabs
        .width
    }

    pub fn sync_document_tabs_scroll(&mut self) {
        crate::widgets::document_tabs::sync_scroll(self, self.document_tabs_area_width());
    }

    pub fn workbench_layout(&self) -> dexo_storage::WorkbenchLayout {
        dexo_storage::WorkbenchLayout {
            version: dexo_storage::LAYOUT_VERSION,
            explorer_visible: self.panes.explorer_visible,
            results_visible: self.panes.results_visible,
            explorer_width: self.panes.explorer_width,
            results_height: self.panes.results_height,
            console_height: self.panes.console_height,
            focused_panel: format!("{:?}", self.focus).to_ascii_lowercase(),
            document_ids: self.documents.iter().map(|d| d.id.clone()).collect(),
            active_document_id: self
                .documents
                .get(self.active_document)
                .map(|d| d.id.clone()),
            active_connection_id: if self.connection.name.is_empty() {
                None
            } else {
                Some(self.connection.name.clone())
            },
            active_result_tab: 0,
        }
    }

    pub fn apply_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.layout_mode = LayoutPlan::for_area_with_document_tabs(
            ratatui::layout::Rect::new(0, 0, width, height),
            Some(&self.effective_panes()),
            true,
        )
        .mode;
        // `self.panes` is what the user set, and is saved as it is: a resize used to
        // clamp it in place, so a pass through a small terminal shrank it for good.
        self.sync_grid_viewport();
        self.sync_document_tabs_scroll();
    }

    /// The pane layout the active document needs. A table document has no editor: the
    /// grid fills the editor's slot and the bottom pane is just a log, so it uses its
    /// own height instead of the one tuned for an editor over a result grid.
    pub fn effective_panes(&self) -> crate::layout::PaneLayout {
        let mut panes = self.panes;
        if self.active_document().kind.is_table() {
            panes.results_height = self.panes.console_height;
        }
        panes.clamp(self.width, self.height)
    }

    /// Switches the active document and brings its output pane with it. Assigning
    /// `active_document` on its own leaves `results_owner` behind, and the next
    /// `update` then parks the pane under the document that no longer owns it.
    /// Drops the placeholder once a real document is open beside it. Every way of
    /// opening one -- Ctrl+N, a file, a table, a connection's console -- pushes onto the
    /// list, and this is the one place that has to know the stand-in should go.
    pub fn drop_redundant_placeholder(&mut self) {
        if self.documents.len() < 2 {
            return;
        }
        let Some(index) = self
            .documents
            .iter()
            .position(|document| document.kind.is_placeholder())
        else {
            return;
        };
        self.documents.remove(index);
        if self.active_document > index {
            self.active_document -= 1;
        }
        self.active_document = self.active_document.min(self.documents.len() - 1);
        self.sync_document_tab_focus();
    }

    /// What is selected in the editor as the user sees it: Vim's Visual selection when
    /// that mode is on, else the editor's own. Running, saving or formatting "the
    /// selection" takes this, not the editor's leftover range under a Visual one.
    pub fn editor_selection(&self) -> Option<std::ops::Range<usize>> {
        crate::screens::vim::display_selection(self)
            .or_else(|| self.active_document().selection())
            .filter(|range| range.start < range.end)
    }

    /// Whether table document `index` has rows staged for the database that are not yet
    /// applied: the tab says so, as it does for text not saved.
    pub fn staged_edits(&self, index: usize) -> bool {
        match self.documents.get(index) {
            Some(document) if document.kind.is_table() && index == self.active_document => {
                self.data.has_pending_edits()
            }
            Some(document) if document.kind.is_table() => document.browse.has_pending_edits(),
            _ => false,
        }
    }

    pub fn set_active_document(&mut self, index: usize) {
        if index >= self.documents.len() {
            return;
        }
        self.active_document = index;
        self.swap_results_to_active_document();
    }

    /// Parks the output pane under the document it belongs to and picks up the active
    /// document's. Results are per document: a query run in one file must not redraw
    /// another file's grid.
    pub fn swap_results_to_active_document(&mut self) -> bool {
        let swapped = self.park_output_under_owner();
        self.adopt_table_target();
        swapped
    }

    fn park_output_under_owner(&mut self) -> bool {
        let Some(active) = self
            .documents
            .get(self.active_document)
            .map(|d| d.id.clone())
        else {
            return false;
        };
        let Some(owner) = self.results_owner.replace(active.clone()) else {
            return false;
        };
        if owner == active {
            return false;
        }
        let parked = std::mem::take(&mut self.results);
        if let Some(previous) = self
            .documents
            .iter_mut()
            .find(|document| document.id == owner)
        {
            previous.results = parked;
            self.data.swap_browse(&mut previous.browse);
        }
        self.results = std::mem::take(&mut self.documents[self.active_document].results);
        let active = &mut self.documents[self.active_document];
        self.data.swap_browse(&mut active.browse);
        true
    }

    /// A table document that never loaded -- new, or restored at boot -- parks the
    /// default state, which names no table; paging it would ask for a table that is not
    /// there.
    fn adopt_table_target(&mut self) {
        let Some(DocumentKind::Table(target)) = self
            .documents
            .get(self.active_document)
            .map(|document| &document.kind)
        else {
            return;
        };
        if self.data.target != *target {
            let mut fresh = crate::screens::data::DataScreen {
                target: target.clone(),
                ..Default::default()
            };
            self.data.swap_browse(&mut fresh);
        }
    }

    pub fn sync_grid_viewport(&mut self) {
        let plan = LayoutPlan::for_area_with_document_tabs(
            ratatui::layout::Rect::new(0, 0, self.width, self.height),
            Some(&self.effective_panes()),
            true,
        );
        let pane = plan.grid_pane(self.active_document().kind.is_table());
        let width = pane.width.saturating_sub(2).max(1);
        let inner_h = pane.height.saturating_sub(2).max(1);
        // The toolbar row used to be drawn only for multiple result sets, and this
        // counted it the same way. It is unconditional now.
        let height = inner_h
            .saturating_sub(crate::widgets::grid::chrome_rows(self))
            .max(1);
        self.results.set_viewport_size(width, height);
    }

    /// Folds a freshly loaded page of catalog objects into what completion reads,
    /// replacing any it already had by id.
    pub fn absorb_catalog(&mut self, objects: &[dexo_driver_api::CatalogObject]) {
        if objects.is_empty() {
            return;
        }
        if self.catalog_connection != self.connection.name {
            self.catalog_objects.clear();
            self.catalog_connection = self.connection.name.clone();
            self.catalog_complete = false;
        }
        self.catalog_revision = self.catalog_revision.wrapping_add(1);
        // Indexed once per page: a whole snapshot arrives at once, and finding each
        // object by scanning the ones already held was quadratic in the database size.
        let mut at: std::collections::HashMap<dexo_driver_api::ObjectId, usize> = self
            .catalog_objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.id.clone(), index))
            .collect();
        for object in objects {
            match at.get(&object.id) {
                Some(&index) => self.catalog_objects[index] = object.clone(),
                None => {
                    at.insert(object.id.clone(), self.catalog_objects.len());
                    self.catalog_objects.push(object.clone());
                }
            }
        }
    }

    pub fn active_document(&self) -> &EditorDocument {
        &self.documents[self
            .active_document
            .min(self.documents.len().saturating_sub(1))]
    }

    pub fn active_document_mut(&mut self) -> &mut EditorDocument {
        let index = self
            .active_document
            .min(self.documents.len().saturating_sub(1));
        &mut self.documents[index]
    }

    pub fn set_sql(&mut self, text: impl AsRef<str>) {
        *self.active_document_mut() = EditorDocument::with_text(text);
        self.editor.reset_parse();
    }
}

#[cfg(test)]
mod editor_document_tests {
    use super::{EditorDocument, describe_native_code};

    #[test]
    fn a_long_line_wraps_at_spaces_and_indents_what_follows() {
        use super::wrap_line;
        assert_eq!(wrap_line("short", 20), ["short"]);
        assert_eq!(
            wrap_line(
                "  CONSTRAINT orders_fkey FOREIGN KEY (customer_id) REFERENCES customers",
                30
            ),
            [
                "  CONSTRAINT orders_fkey",
                "    FOREIGN KEY (customer_id)",
                "    REFERENCES customers"
            ]
        );
        // No space to break at: inside the word.
        assert_eq!(
            wrap_line("abcdefghijklmnopqrstuvwxyz", 10),
            ["abcdefghij", "  klmnopqr", "  stuvwxyz"]
        );
    }

    /// SQLite's result code `1` was printed as `SQLSTATE 1`.
    #[test]
    fn a_driver_code_is_named_for_what_it_is() {
        assert_eq!(describe_native_code("42P01"), "SQLSTATE 42P01");
        assert_eq!(
            describe_native_code("1146 (42S02)"),
            "error 1146 · SQLSTATE 42S02"
        );
        assert_eq!(describe_native_code("1"), "error code 1");
        assert_eq!(describe_native_code("2067"), "error code 2067");
        assert_eq!(describe_native_code("Catalog"), "Catalog");
    }

    #[test]
    fn new_documents_get_unique_ids_and_connection() {
        let a = EditorDocument::new_unique("console.sql", None, Some("conn-a".into()));
        let b = EditorDocument::new_unique("q2.sql", None, Some("conn-a".into()));
        assert_ne!(a.id, b.id);
        assert_eq!(a.connection_id.as_deref(), Some("conn-a"));
        assert_eq!(a.title, "console.sql");
    }
}
