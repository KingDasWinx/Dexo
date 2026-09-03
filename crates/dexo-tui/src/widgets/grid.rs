use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::model::{Focus, Model, allocate_column_widths, format_value, truncate_cell};
use crate::mouse::{HitMap, HitTarget};
use crate::theme::Role;

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if area.width < 2 || area.height < 2 {
        frame.render_widget(Paragraph::new(preview_lines(model, area, hits)), area);
        return;
    }
    let extra = result_banner(model);
    let title = if model.results.truncated() {
        format!("Results ({}) …{extra}", model.results.row_count())
    } else {
        format!("Results ({}){extra}", model.results.row_count())
    };
    let focused = model.focus == Focus::Results;
    let block = crate::render::pane_block(model, &title, focused);
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
    frame.render_widget(Paragraph::new(preview_lines(model, body, hits)), body);
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

fn preview_lines(model: &Model, area: Rect, hits: &mut HitMap) -> Vec<Line<'static>> {
    let grid = &model.results;
    let col_indices = grid.visible_column_indices();
    let widths = grid.column_widths();
    let natural_widths: Vec<u16> = col_indices
        .iter()
        .map(|&index| widths.get(index).copied().unwrap_or(8))
        .collect();
    let (cell_widths, overflowed) =
        allocate_column_widths(&natural_widths, area.width as usize);
    let mut header = Vec::new();
    let mut remaining = area.width as usize;
    let header_style = model.theme.header(model.capabilities);
    for (&index, &width) in col_indices.iter().zip(cell_widths.iter()) {
        let Some(column) = grid.columns().get(index) else {
            continue;
        };
        let cell_width = (width as usize).min(remaining);
        let header_x = area
            .x
            .saturating_add((area.width as usize - remaining) as u16);
        hits.register(
            HitTarget::GridHeader(index),
            Rect::new(header_x, area.y, cell_width as u16, 1),
        );
        header.push(Span::styled(
            format!(
                "{:width$}",
                truncate_cell(&column.name, cell_width),
                width = cell_width
            ),
            header_style,
        ));
        remaining = remaining.saturating_sub(cell_width);
        if remaining > 0 {
            header.push(Span::raw(" "));
            remaining = remaining.saturating_sub(1);
        }
    }
    if overflowed {
        header.push(Span::styled("…", header_style));
    }
    let mut lines = vec![Line::from(header)];
    let body_height = area.height.saturating_sub(1) as usize;
    let sel_marker = crate::accessibility::marker(Role::Selection, model.capabilities.unicode);
    let active_style = model.theme.active_row(model.capabilities);
    let selected_style = model.theme.selected_row(model.capabilities);
    let cursor_row = grid.cursor_row();
    for (visible_i, row) in grid
        .visible_slice(grid.viewport().row_offset, body_height)
        .into_iter()
        .enumerate()
    {
        let hit_y = area.y.saturating_add(1).saturating_add(visible_i as u16);
        if hit_y < area.y.saturating_add(area.height) {
            hits.register(
                HitTarget::GridRow(row.source_index),
                Rect::new(area.x, hit_y, area.width, 1),
            );
        }
        let mut remaining = area.width as usize;
        let mut spans = Vec::new();
        let is_active = cursor_row == Some(row.source_index);
        let is_sel = grid.row_selected(row.source_index);
        let row_style = if is_active {
            active_style
        } else if is_sel {
            selected_style
        } else {
            model
                .theme
                .zebra(row.source_index % 2 == 1, model.capabilities)
        };
        let (row_widths, row_overflowed) = if is_active || is_sel {
            spans.push(Span::styled(format!("{sel_marker} "), row_style));
            remaining = remaining.saturating_sub(sel_marker.chars().count() + 1);
            allocate_column_widths(&natural_widths, remaining)
        } else {
            (cell_widths.clone(), overflowed)
        };
        let mut cell_x = area
            .x
            .saturating_add((area.width as usize - remaining) as u16);
        for (&index, &width) in col_indices.iter().zip(row_widths.iter()) {
            let Some(value) = row.cells.get(index) else {
                continue;
            };
            let cell_width = (width as usize).min(remaining);
            hits.register(
                HitTarget::GridCell {
                    row: row.source_index,
                    col: index,
                },
                Rect::new(cell_x, hit_y, cell_width as u16, 1),
            );
            spans.push(Span::styled(
                format!(
                    "{:width$}",
                    truncate_cell(&format_value(value), cell_width),
                    width = cell_width
                ),
                row_style,
            ));
            remaining = remaining.saturating_sub(cell_width);
            cell_x = cell_x.saturating_add(cell_width as u16);
            if remaining > 0 {
                spans.push(Span::styled(" ", row_style));
                remaining = remaining.saturating_sub(1);
                cell_x = cell_x.saturating_add(1);
            }
        }
        if row_overflowed {
            spans.push(Span::styled("…", row_style));
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
    fn narrow_columns_are_not_sacrificed_for_wide_ones() {
        use crate::model::allocate_column_widths;

        // A wide "description" column followed by a short "id" column: the
        // id must keep its natural width instead of being clipped just
        // because it comes after a column that doesn't fit.
        let natural = vec![40u16, 2];
        let (widths, overflowed) = allocate_column_widths(&natural, 20);
        assert_eq!(widths[1], 2, "short trailing column must render in full");
        assert!(widths[0] < 40, "the wide column must absorb the shrinkage");
        assert!(!overflowed);
    }

    #[test]
    fn scrolling_widens_a_column_for_longer_values_further_down() {
        use dexo_driver_api::{ColumnMeta, DbValue};

        let mut grid = GridModel::default();
        grid.set_columns(vec![ColumnMeta {
            name: "id".into(),
            type_name: "int8".into(),
            nullable: false,
        }]);
        grid.append_rows((1..=200).map(|id| vec![DbValue::I64(id)]).collect());
        grid.set_viewport_size(40, 10);
        assert_eq!(grid.column_widths()[0], 2, "first rows only need two digits");

        grid.scroll_rows(120);
        assert_eq!(
            grid.column_widths()[0],
            3,
            "three-digit ids must not be clipped once they scroll into view"
        );

        grid.scroll_rows(-120);
        assert_eq!(
            grid.column_widths()[0],
            3,
            "widths must not shrink back and make the grid jitter"
        );
    }

    #[test]
    fn allocate_column_widths_returns_natural_widths_when_everything_fits() {
        use crate::model::allocate_column_widths;

        let natural = vec![3u16, 5, 2];
        let (widths, overflowed) = allocate_column_widths(&natural, 40);
        assert_eq!(widths, natural);
        assert!(!overflowed);
    }

    #[test]
    fn allocate_column_widths_drops_trailing_columns_when_none_fit() {
        use crate::model::allocate_column_widths;

        let natural = vec![10u16; 10];
        let (widths, overflowed) = allocate_column_widths(&natural, 5);
        assert!(widths.len() < natural.len());
        assert!(overflowed);
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

    #[test]
    fn cursor_moves_and_shift_extends_range() {
        use crate::model::GridSelection;

        let mut grid = GridModel::sample_rows(8);
        grid.set_viewport_size(40, 4);
        grid.move_cursor_row(2, false);
        assert_eq!(grid.cursor_row(), Some(2));
        grid.move_cursor_row(1, true);
        assert!(matches!(
            grid.kind,
            GridSelection::Range {
                start: (2, _),
                end: (3, _)
            }
        ));
        assert!(grid.row_selected(2));
        assert!(grid.row_selected(3));
        assert!(!grid.row_selected(0));
    }

    #[test]
    fn last_row_stays_visible_when_cursor_reaches_bottom() {
        let mut grid = GridModel::sample_rows(20);
        // height here is data rows only (header already reserved by sync_grid_viewport).
        grid.set_viewport_size(40, 5);
        grid.select_cell(0, 0);
        for _ in 0..19 {
            grid.move_cursor_row(1, false);
        }
        assert_eq!(grid.cursor_row(), Some(19));
        let visible = grid.visible_slice(grid.viewport().row_offset, grid.viewport().height);
        assert!(
            visible.iter().any(|row| row.source_index == 19),
            "last row must remain in the painted data window: offset={} height={} visible={:?}",
            grid.viewport().row_offset,
            grid.viewport().height,
            visible
                .iter()
                .map(|row| row.source_index)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn sync_grid_viewport_reserves_header_from_results_pane() {
        use crate::layout::LayoutPlan;
        use crate::model::Model;

        let mut model = Model::default();
        model.apply_size(120, 40);
        let plan = LayoutPlan::for_area_with(
            ratatui::layout::Rect::new(0, 0, model.width, model.height),
            Some(&model.panes),
        );
        let inner_h = plan.results.height.saturating_sub(2).max(1);
        let expected = inner_h.saturating_sub(1).max(1) as usize; // column header
        assert_eq!(
            model.results.viewport().height, expected,
            "viewport height must match painted data rows, not the full pane inner height"
        );
    }

    #[test]
    fn ctrl_pick_copies_noncontiguous_rows() {
        use dexo_app::data::{CopyFormat, SqlDialect};

        let mut grid = GridModel::sample_rows(6);
        grid.select_cell(0, 0);
        grid.toggle_picked_row();
        grid.move_cursor_row(2, false);
        grid.toggle_picked_row();
        assert!(grid.row_selected(0));
        assert!(grid.row_selected(2));
        assert!(!grid.picked_rows.contains(&1));
        let json = grid.copy(CopyFormat::Json, SqlDialect::Postgres).unwrap();
        assert!(json.contains("\"n\": 0"));
        assert!(json.contains("\"n\": 2"));
        assert!(!json.contains("\"n\": 1"));
    }

    #[test]
    fn left_right_pans_to_hidden_columns() {
        use crate::model::GridSelection;
        use dexo_driver_api::{ColumnMeta, DbValue};

        let mut grid = GridModel::default();
        grid.set_columns(
            (0..8)
                .map(|i| ColumnMeta {
                    name: format!("wide_column_name_{i:02}"),
                    type_name: "text".into(),
                    nullable: true,
                })
                .collect(),
        );
        grid.append_rows(vec![
            (0..8).map(|_| DbValue::Text("x".repeat(40))).collect(),
        ]);
        grid.set_viewport_size(20, 4);
        grid.select_row(0);
        assert_eq!(grid.viewport().column_offset, 0);
        for expected in 1..=4 {
            grid.move_cursor_col(1);
            assert_eq!(
                grid.viewport().column_offset,
                expected,
                "right should increment column_offset every key when columns overflow"
            );
            assert!(
                matches!(grid.kind, GridSelection::Row { row: 0 }),
                "left/right must keep the row cursor"
            );
        }
        grid.move_cursor_row(1, true);
        let before = grid.kind.clone();
        grid.move_cursor_col(1);
        assert_eq!(
            std::mem::discriminant(&grid.kind),
            std::mem::discriminant(&before),
            "left/right must not extend or collapse a row range"
        );
        assert!(grid.viewport().column_offset > 4);
        while grid.viewport().column_offset > 0 {
            grid.move_cursor_col(-1);
        }
        assert_eq!(grid.viewport().column_offset, 0);
    }
}
