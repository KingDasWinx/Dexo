use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::model::{Model, format_value, truncate_cell};
use crate::mouse::{HitMap, HitTarget};
use crate::theme::Role;

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if area.width < 2 || area.height < 2 {
        frame.render_widget(Paragraph::new(preview_lines(model, area)), area);
        return;
    }
    let extra = result_banner(model);
    let title = if model.results.truncated() {
        format!("Results ({}) …{extra}", model.results.row_count())
    } else {
        format!("Results ({}){extra}", model.results.row_count())
    };
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let tab_h = if model.results.tabs.len() > 1 { 1 } else { 0 };
    if tab_h > 0 {
        let tabs = Rect::new(inner.x, inner.y, inner.width, 1);
        let mut x = tabs.x;
        for (index, tab) in model.results.tabs.iter().enumerate() {
            let label = format!(" {} ", tab.title);
            let width = label.len() as u16;
            let rect = Rect::new(
                x,
                tabs.y,
                width.min(tabs.width.saturating_sub(x.saturating_sub(tabs.x))),
                1,
            );
            hits.register(HitTarget::ResultTab(index), rect);
            x = x.saturating_add(width);
        }
        frame.render_widget(
            Paragraph::new(
                model
                    .results
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(index, tab)| {
                        if index == model.results.active {
                            format!("[{}]", tab.title)
                        } else {
                            format!(" {} ", tab.title)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(""),
            ),
            tabs,
        );
    }
    let body = Rect::new(
        inner.x,
        inner.y.saturating_add(tab_h),
        inner.width,
        inner.height.saturating_sub(tab_h),
    );
    frame.render_widget(Paragraph::new(preview_lines(model, body)), body);
}

fn result_banner(model: &Model) -> String {
    let mut extra = String::new();
    if let Some(tab) = model.results.tabs.get(model.results.active) {
        if !tab.notices.is_empty() {
            extra.push_str(" !");
            extra.push_str(tab.notices.last().expect("notice"));
        }
        if let Some(reason) = &tab.local_only {
            extra.push_str(" local-only:");
            extra.push_str(reason);
        }
    }
    if !model.data.crumbs.is_empty() {
        extra.push_str(" crumbs:");
        extra.push_str(&model.data.crumbs.len().to_string());
    }
    if model.data.has_more {
        extra.push_str(" more");
    }
    extra
}

fn preview_lines(model: &Model, area: Rect) -> Vec<Line<'static>> {
    let grid = &model.results;
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
    let selected = grid.selection();
    let sel_marker = crate::accessibility::marker(Role::Selection, model.capabilities.unicode);
    let sel_style = model.theme.style(Role::Selection, model.capabilities);
    for row in grid.visible_slice(grid.viewport().row_offset, body_height) {
        let mut remaining = area.width as usize;
        let mut spans = Vec::new();
        let is_sel = selected.is_some_and(|(r, _)| r == row.source_index);
        if is_sel {
            spans.push(Span::styled(format!("{sel_marker} "), sel_style));
            remaining = remaining.saturating_sub(sel_marker.chars().count() + 1);
        }
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
            let style = if is_sel { sel_style } else { Style::default() };
            spans.push(Span::styled(
                format!(
                    "{:width$}",
                    truncate_cell(&format_value(value), cell_width),
                    width = cell_width
                ),
                style,
            ));
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
        let grid = GridModel::sample_rows(100_000).with_viewport(50_000, 20);
        let rendered = grid.visible_rows();
        assert_eq!(rendered.len(), 20);
        assert_eq!(rendered[0].source_index, 50_000);
    }

    #[test]
    fn viewport_smoke_100k_rows() {
        let grid = GridModel::sample_rows(100_000).with_viewport(99_980, 20);
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

        let mut grid = GridModel::sample_rows(4);
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
