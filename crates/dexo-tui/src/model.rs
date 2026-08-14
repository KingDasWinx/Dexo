use dexo_app::event::TaskId;
use dexo_app::{ExecutionTarget, ScriptPolicy};
use dexo_driver_api::{ColumnMeta, DbValue, QueryId, TransactionState};

use crate::layout::{LayoutMode, LayoutPlan};
use crate::screens::admin::AdminScreen;
use crate::screens::data::DataScreen;
use crate::screens::explain::ExplainScreen;
use crate::screens::explorer::ExplorerState;
use crate::screens::mcp_profiles::McpProfilesScreen;
use crate::screens::schema_diff::SchemaDiffScreen;
use crate::screens::schema_editor::SchemaEditor;
use crate::screens::security::SecurityScreen;
use crate::screens::transfer::TransferScreen;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    Explorer,
    Editor,
    Results,
    Inspector,
    Palette,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionStatus {
    pub name: String,
    pub ready: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
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
}

impl GridModel {
    pub fn fixture_rows(count: usize) -> Self {
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
    }

    pub fn append_rows(&mut self, rows: Vec<Vec<DbValue>>) {
        self.buffer.append_rows(rows);
        self.recompute_column_widths();
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.viewport.row_offset = 0;
        self.viewport.column_offset = 0;
        self.selection = None;
        self.column_widths.clear();
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
        self.selection = Some(start);
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

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub focus: Focus,
    pub width: u16,
    pub height: u16,
    pub layout_mode: LayoutMode,
    pub connection: ConnectionStatus,
    pub transaction: TransactionState,
    pub results: GridModel,
    pub tabs: TabsState,
    pub palette: PaletteState,
    pub messages: Vec<String>,
    pub sql: String,
    pub cursor: usize,
    pub selection: Option<std::ops::Range<usize>>,
    pub result_tabs: Vec<GridModel>,
    pub execution_target: ExecutionTarget,
    pub script_policy: ScriptPolicy,
    pub active_task: Option<TaskId>,
    pub active_query: Option<QueryId>,
    pub project: String,
    pub schema: String,
    pub explorer: ExplorerState,
    pub data: DataScreen,
    pub schema_editor: SchemaEditor,
    pub schema_diff: SchemaDiffScreen,
    pub transfer: TransferScreen,
    pub security: SecurityScreen,
    pub explain: ExplainScreen,
    pub admin: AdminScreen,
    pub mcp_profiles: McpProfilesScreen,
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
            },
            transaction: TransactionState::Idle,
            results: GridModel::default(),
            tabs: TabsState::default(),
            palette: PaletteState::default(),
            messages: Vec::new(),
            sql: String::new(),
            cursor: 0,
            selection: None,
            result_tabs: Vec::new(),
            execution_target: ExecutionTarget::Document,
            script_policy: ScriptPolicy::StopOnError,
            active_task: None,
            active_query: None,
            project: "default".into(),
            schema: String::new(),
            explorer: ExplorerState::default(),
            data: DataScreen::default(),
            schema_editor: SchemaEditor::default(),
            schema_diff: SchemaDiffScreen::default(),
            transfer: TransferScreen::default(),
            security: SecurityScreen::default(),
            explain: ExplainScreen::default(),
            admin: AdminScreen::default(),
            mcp_profiles: McpProfilesScreen::default(),
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

    pub fn apply_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.layout_mode =
            LayoutPlan::for_area(ratatui::layout::Rect::new(0, 0, width, height)).mode;
    }
}
