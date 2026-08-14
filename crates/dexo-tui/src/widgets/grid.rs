use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::model::{GridModel, Model, format_value, truncate_cell};

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if area.width < 2 || area.height < 2 {
        frame.render_widget(Paragraph::new(preview_lines(&model.results, area)), area);
        return;
    }
    let title = if model.results.truncated() {
        format!("Results ({}) …", model.results.row_count())
    } else {
        format!("Results ({})", model.results.row_count())
    };
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    frame.render_widget(Paragraph::new(preview_lines(&model.results, inner)), inner);
}

fn preview_lines(grid: &GridModel, area: Rect) -> Vec<Line<'static>> {
    let col_indices = grid.visible_column_indices();
    let widths = grid.column_widths();
    let mut header = Vec::new();
    let mut remaining = area.width as usize;
    for index in col_indices {
        let Some(column) = grid.columns().get(index) else {
            continue;
        };
        if remaining == 0 {
            header.push(Span::raw("…"));
            break;
        }
        let width = widths.get(index).copied().unwrap_or(8) as usize;
        let cell_width = width.min(remaining);
        header.push(Span::raw(format!(
            "{:width$}",
            truncate_cell(&column.name, cell_width),
            width = cell_width
        )));
        remaining = remaining.saturating_sub(cell_width + 1);
        if remaining > 0 {
            header.push(Span::raw(" "));
            remaining = remaining.saturating_sub(1);
        }
    }
    let mut lines = vec![Line::from(header)];
    let body_height = area.height.saturating_sub(1) as usize;
    for row in grid.visible_slice(grid.viewport().row_offset, body_height) {
        let mut remaining = area.width as usize;
        let mut spans = Vec::new();
        for index in grid.visible_column_indices() {
            let Some(value) = row.cells.get(index) else {
                continue;
            };
            if remaining == 0 {
                spans.push(Span::raw("…"));
                break;
            }
            let width = widths.get(index).copied().unwrap_or(8) as usize;
            let cell_width = width.min(remaining);
            spans.push(Span::raw(format!(
                "{:width$}",
                truncate_cell(&format_value(value), cell_width),
                width = cell_width
            )));
            remaining = remaining.saturating_sub(cell_width + 1);
            if remaining > 0 {
                spans.push(Span::raw(" "));
                remaining = remaining.saturating_sub(1);
            }
        }
        lines.push(Line::from(spans));
    }
    lines
}

#[cfg(test)]
mod tests {
    use crate::model::{GridModel, truncate_cell};

    #[test]
    fn renders_only_visible_rows() {
        let grid = GridModel::fixture_rows(100_000).with_viewport(50_000, 20);
        let rendered = grid.visible_rows();
        assert_eq!(rendered.len(), 20);
        assert_eq!(rendered[0].source_index, 50_000);
    }

    #[test]
    fn viewport_smoke_100k_rows() {
        let grid = GridModel::fixture_rows(100_000).with_viewport(99_980, 20);
        let rendered = grid.visible_rows();
        assert_eq!(rendered.len(), 20);
        assert_eq!(rendered[0].source_index, 99_980);
        assert_eq!(rendered[19].source_index, 99_999);
        assert!(
            std::mem::size_of_val(rendered.as_slice()) < 8_000,
            "visible_rows must not allocate offscreen row data"
        );
    }

    #[test]
    fn truncation_marker_when_cell_overflows() {
        assert_eq!(truncate_cell("abcdefghij", 4), "abc…");
    }

    #[test]
    fn selection_copy_freeze_and_hide() {
        use crate::model::GridSelection;
        use dexo_app::data::{CopyFormat, SqlDialect};

        let mut grid = GridModel::fixture_rows(4);
        grid.select_cell(2, 0);
        assert!(matches!(grid.kind, GridSelection::Cell { row: 2, col: 0 }));
        let text = grid.copy(CopyFormat::Text, SqlDialect::Postgres).unwrap();
        assert!(text.contains('2'));
        grid.select_row(1);
        let csv = grid.copy(CopyFormat::Csv, SqlDialect::Postgres).unwrap();
        assert!(csv.contains("n"));
        grid.select_column(0);
        let json = grid.copy(CopyFormat::Json, SqlDialect::Postgres).unwrap();
        assert!(json.contains('0'));
        grid.select_range((0, 0), (1, 0));
        let md = grid
            .copy(CopyFormat::Markdown, SqlDialect::Postgres)
            .unwrap();
        assert!(md.contains('|'));
        grid.freeze_columns(1);
        grid.hide_column(0);
        assert!(grid.visible_column_indices().is_empty());
        grid.hidden_columns.clear();
        let sql = grid.copy(CopyFormat::Sql, SqlDialect::Mysql).unwrap();
        assert!(sql.contains('`'));
    }
}
