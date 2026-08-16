use std::ops::Range;

use dexo_app::event::TaskId;
use dexo_app::{ExecutionTarget, ScriptPolicy};
use dexo_driver_api::{ColumnMeta, DbValue, QueryId, TransactionState};
use dexo_sql::SqlDocument;

use crate::runtime::{OperationId, OperationKey, SessionId};

use crate::capabilities::TerminalCapabilities;
use crate::keymap::{Chord, Keymap};
use crate::layout::{LayoutMode, LayoutPlan, LayoutPreset, PaneLayout};
use crate::mouse::HitMap;
use crate::screens::admin::AdminScreen;
use crate::screens::config_transfer::ConfigTransferScreen;
use crate::screens::connection::ConnectionForm;
use crate::screens::connections::ConnectionsScreen;
use crate::screens::data::DataScreen;
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
use crate::screens::transfer::TransferScreen;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    Explorer,
    Editor,
    Results,
    Inspector,
    Palette,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectionStatus {
    pub name: String,
    pub ready: bool,
    pub environment: String,
    pub read_only: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HelpState {
    pub open: bool,
    pub scroll: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResultsMenuState {
    pub open: bool,
    pub selected: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabsState {
    pub active: usize,
    pub titles: Vec<String>,
}

impl Default for TabsState {
    fn default() -> Self {
        Self {
            active: 0,
            titles: vec![
                "SQL".into(),
                "Data".into(),
                "DDL".into(),
                "Properties".into(),
                "Explain".into(),
            ],
        }
    }
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
    rows: Vec<Vec<DbValue>>,
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

    pub fn estimated_bytes(&self) -> usize {
        self.estimated_bytes
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn append_rows(&mut self, rows: Vec<Vec<DbValue>>) {
        for row in rows {
            let added = estimated_row_bytes(&row);
            if self.rows.len() >= Self::MAX_ROWS
                || self.estimated_bytes.saturating_add(added) > Self::MAX_BYTES
            {
                self.truncated = true;
                break;
            }
            self.estimated_bytes += added;
            self.rows.push(row);
        }
    }

    pub fn clear(&mut self) {
        self.columns.clear();
        self.rows.clear();
        self.estimated_bytes = 0;
        self.truncated = false;
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridModel {
    buffer: ResultBuffer,
    viewport: GridViewport,
    selection: Option<(usize, usize)>,
    column_widths: Vec<u16>,
    pub kind: GridSelection,
    pub frozen_columns: usize,
    pub hidden_columns: Vec<usize>,
    pub cells: std::collections::BTreeMap<(usize, usize), GridCell>,
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
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResultsState {
    pub tabs: Vec<ResultTab>,
    pub active: usize,
}

impl ResultsState {
    fn grid(&self) -> &GridModel {
        self.tabs
            .get(self.active)
            .map(|tab| &tab.grid)
            .unwrap_or(&EMPTY_GRID)
    }

    fn grid_mut(&mut self) -> &mut GridModel {
        if self.tabs.is_empty() {
            self.tabs.push(ResultTab::new(
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

static EMPTY_GRID: GridModel = GridModel {
    buffer: ResultBuffer {
        columns: Vec::new(),
        rows: Vec::new(),
        estimated_bytes: 0,
        truncated: false,
    },
    viewport: GridViewport {
        row_offset: 0,
        column_offset: 0,
        height: 20,
        width: 80,
    },
    selection: None,
    column_widths: Vec::new(),
    kind: GridSelection::Cell { row: 0, col: 0 },
    frozen_columns: 0,
    hidden_columns: Vec::new(),
    cells: std::collections::BTreeMap::new(),
};

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
            frozen_columns: 0,
            hidden_columns: Vec::new(),
            cells: std::collections::BTreeMap::new(),
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
        self.column_widths.clear();
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
    }

    pub fn scroll_rows(&mut self, delta: i32) {
        let next = self.viewport.row_offset as i32 + delta;
        self.viewport.row_offset = next.max(0) as usize;
        self.clamp_scroll();
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

    pub fn select_row(&mut self, row: usize) {
        self.kind = GridSelection::Row { row };
        self.selection = Some((row, 0));
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
            self.select_cell(0, 0);
        }
    }

    pub fn cursor_row(&self) -> Option<usize> {
        self.selection.map(|(row, _)| row)
    }

    pub fn row_selected(&self, row: usize) -> bool {
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

    pub fn move_cursor_row(&mut self, delta: i32, extend: bool) {
        self.ensure_cursor();
        let Some((row, col)) = self.selection else {
            return;
        };
        let last = self.buffer.row_count().saturating_sub(1);
        let next = (row as i32 + delta).clamp(0, last as i32) as usize;
        if extend {
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

    pub fn move_cursor_col(&mut self, delta: i32) {
        match self.kind {
            // ponytail: H-scroll is column_offset pan (pre-row-cursor). Ceiling: no sticky column cursor. Add one if cell-edit lands.
            GridSelection::Row { .. } | GridSelection::Range { .. } => self.scroll_columns(delta),
            GridSelection::Cell { .. } | GridSelection::Column { .. } => {
                self.ensure_cursor();
                let Some((row, col)) = self.selection else {
                    return;
                };
                let last = self.buffer.columns.len().saturating_sub(1);
                let next = (col as i32 + delta).clamp(0, last as i32) as usize;
                match &mut self.kind {
                    GridSelection::Cell { col, .. } | GridSelection::Column { col } => *col = next,
                    GridSelection::Row { .. } | GridSelection::Range { .. } => {}
                }
                self.selection = Some((row, next));
                self.scroll_columns(delta);
            }
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
        let (columns, rows) = self.selected_matrix();
        dexo_app::data::copy_selection(&columns, &rows, format, dialect)
    }

    fn selected_matrix(&self) -> (Vec<String>, Vec<Vec<DbValue>>) {
        let cols: Vec<usize> = match self.kind {
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
        let row_idxs: Vec<usize> = match self.kind {
            GridSelection::Cell { row, .. } | GridSelection::Row { row } => vec![row],
            GridSelection::Column { .. } => (0..self.buffer.row_count()).collect(),
            GridSelection::Range { start, end } => {
                let lo = start.0.min(end.0);
                let hi = start.0.max(end.0);
                (lo..=hi).collect()
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
                                unicode_width::UnicodeWidthStr::width(format_value(value).as_str())
                                    as u16
                            })
                            .unwrap_or(0)
                    })
                    .max()
                    .unwrap_or(0);
                header.max(body).clamp(1, 40)
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

#[derive(Clone, Debug)]
pub struct EditorDocument {
    pub id: String,
    pub title: String,
    pub path: Option<std::path::PathBuf>,
    pub sql: SqlDocument,
    pub saved_revision: u64,
    pub session: Option<SessionId>,
    pub viewport_line: usize,
    pub viewport_column: usize,
    pub typing: bool,
    pub anchor: Option<usize>,
}

impl PartialEq for EditorDocument {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.title == other.title
            && self.path == other.path
            && self.sql.text() == other.sql.text()
            && self.sql.cursor() == other.sql.cursor()
            && self.saved_revision == other.saved_revision
            && self.session == other.session
            && self.viewport_line == other.viewport_line
            && self.viewport_column == other.viewport_column
            && self.typing == other.typing
            && self.anchor == other.anchor
    }
}

impl EditorDocument {
    pub fn scratch() -> Self {
        Self {
            id: "scratch".into(),
            title: "scratch.sql".into(),
            path: None,
            sql: SqlDocument::new(""),
            saved_revision: 0,
            session: None,
            viewport_line: 0,
            viewport_column: 0,
            typing: false,
            anchor: None,
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

    pub fn is_dirty(&self) -> bool {
        self.sql.revision() != self.saved_revision
    }

    pub fn text(&self) -> String {
        self.sql.text()
    }

    pub fn cursor(&self) -> usize {
        self.sql.cursor()
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

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub focus: Focus,
    pub width: u16,
    pub height: u16,
    pub layout_mode: LayoutMode,
    pub connection: ConnectionStatus,
    pub transaction: TransactionState,
    pub results: ResultsState,
    pub tabs: TabsState,
    pub palette: PaletteState,
    pub help: HelpState,
    pub results_menu: ResultsMenuState,
    pub layout_preset: LayoutPreset,
    pub messages: Vec<String>,
    pub documents: Vec<EditorDocument>,
    pub active_document: usize,
    pub execution_target: ExecutionTarget,
    pub script_policy: ScriptPolicy,
    pub active_task: Option<TaskId>,
    pub active_query: Option<QueryId>,
    pub active_operation: Option<OperationId>,
    pub active_session: Option<SessionId>,
    pub session_generation: u64,
    pub connect_token: u64,
    pub project: String,
    pub project_id: String,
    pub schema: String,
    pub explorer: ExplorerState,
    pub inspector: ObjectInspector,
    pub data: DataScreen,
    pub schema_editor: SchemaEditor,
    pub schema_diff: SchemaDiffScreen,
    pub transfer: TransferScreen,
    pub security: SecurityScreen,
    pub explain: ExplainScreen,
    pub admin: AdminScreen,
    pub mcp_profiles: McpProfilesScreen,
    pub connection_form: ConnectionForm,
    pub connections: ConnectionsScreen,
    pub projects: ProjectsScreen,
    pub config_transfer: ConfigTransferScreen,
    pub secret_prompt: SecretPrompt,
    pub settings: SettingsScreen,
    pub recovery: RecoveryScreen,
    pub mcp_audit: McpAuditScreen,
    pub editor: EditorState,
    pub theme: Theme,
    pub capabilities: TerminalCapabilities,
    pub keymap: Keymap,
    pub pending_chord: Chord,
    pub panes: PaneLayout,
    pub mouse: bool,
    pub animation: bool,
    pub layout_dirty: bool,
    pub hits: HitMap,
    pub file_picker: FilePicker,
    pub file_picker_mode: FilePickerMode,
    pub diagnostic_preview: Option<String>,
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
            },
            theme: crate::theme::builtin_dark(),
            capabilities: TerminalCapabilities {
                color_depth: crate::capabilities::ColorDepth::TrueColor,
                unicode: true,
                mouse: true,
            },
            keymap: Keymap::default_profile(),
            pending_chord: Chord { keys: Vec::new() },
            help: HelpState::default(),
            results_menu: ResultsMenuState::default(),
            layout_preset: LayoutPreset::Normal,
            panes: PaneLayout {
                explorer_visible: true,
                inspector_visible: true,
                results_visible: true,
                explorer_width: 28,
                inspector_width: 28,
                results_height: 12,
            },
            mouse: true,
            animation: true,
            layout_dirty: false,
            hits: HitMap::default(),
            file_picker: FilePicker::default(),
            file_picker_mode: FilePickerMode::Open,
            diagnostic_preview: None,
            transaction: TransactionState::Idle,
            results: ResultsState::default(),
            tabs: TabsState::default(),
            palette: PaletteState::default(),
            messages: Vec::new(),
            documents: vec![EditorDocument::scratch()],
            active_document: 0,
            execution_target: ExecutionTarget::Document,
            script_policy: ScriptPolicy::StopOnError,
            active_task: None,
            active_query: None,
            active_operation: None,
            active_session: None,
            session_generation: 0,
            connect_token: 0,
            project: "default".into(),
            project_id: String::new(),
            schema: String::new(),
            explorer: ExplorerState::default(),
            inspector: ObjectInspector::default(),
            data: DataScreen::default(),
            schema_editor: SchemaEditor::default(),
            schema_diff: SchemaDiffScreen::default(),
            transfer: TransferScreen::default(),
            security: SecurityScreen::default(),
            explain: ExplainScreen::default(),
            admin: AdminScreen::default(),
            mcp_profiles: McpProfilesScreen::default(),
            connection_form: ConnectionForm::default(),
            connections: ConnectionsScreen::default(),
            projects: ProjectsScreen::default(),
            config_transfer: ConfigTransferScreen::default(),
            secret_prompt: SecretPrompt::default(),
            settings: SettingsScreen::default(),
            recovery: RecoveryScreen::default(),
            mcp_audit: McpAuditScreen::default(),
            editor: EditorState::default(),
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

    pub fn workbench_layout(&self) -> dexo_storage::WorkbenchLayout {
        dexo_storage::WorkbenchLayout {
            version: dexo_storage::LAYOUT_VERSION,
            explorer_visible: self.panes.explorer_visible,
            inspector_visible: self.panes.inspector_visible,
            results_visible: self.panes.results_visible,
            explorer_width: self.panes.explorer_width,
            inspector_width: self.panes.inspector_width,
            results_height: self.panes.results_height,
            focused_panel: format!("{:?}", self.focus).to_ascii_lowercase(),
            active_tab: self.tabs.active,
            tabs: self.tabs.titles.clone(),
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
        self.layout_mode = LayoutPlan::for_area_with(
            ratatui::layout::Rect::new(0, 0, width, height),
            Some(&self.panes.clamp(width, height)),
        )
        .mode;
        self.panes = self.panes.clamp(width, height);
        self.sync_grid_viewport();
    }

    pub fn sync_grid_viewport(&mut self) {
        let plan = LayoutPlan::for_area_with(
            ratatui::layout::Rect::new(0, 0, self.width, self.height),
            Some(&self.panes),
        );
        let width = plan.results.width.saturating_sub(2).max(1);
        let height = plan.results.height.saturating_sub(2).max(1);
        self.results.set_viewport_size(width, height);
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
