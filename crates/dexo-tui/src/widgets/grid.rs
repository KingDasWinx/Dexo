use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::model::{
    Focus, Model, ResultsView, Severity, allocate_column_widths, format_value, truncate_cell,
};
use crate::mouse::{HitMap, HitTarget};
use crate::theme::Role;

/// Rows the grid spends on chrome inside its border: the toolbar, then the column
/// header. `Model::sync_grid_viewport` sizes the row viewport against this, and the two
/// must agree -- believing in one row more than the pane draws walks the cursor off the
/// bottom, where the selection is invisible.
pub const CHROME_ROWS: u16 = TOOLBAR_ROWS + HEADER_ROWS;
const TOOLBAR_ROWS: u16 = 1;
const HEADER_ROWS: u16 = 1;

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
    let focused = model.effective_focus() == Focus::Results;
    let block = crate::render::pane_block(model, &title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let toolbar = Rect::new(inner.x, inner.y, inner.width, TOOLBAR_ROWS);
    frame.render_widget(
        Paragraph::new(output_toolbar(model, hits, toolbar)),
        toolbar,
    );
    let body = Rect::new(
        inner.x,
        inner.y.saturating_add(TOOLBAR_ROWS),
        inner.width,
        inner.height.saturating_sub(TOOLBAR_ROWS),
    );
    match model.results.view {
        ResultsView::Explain => {
            let plan = model.explain.lines().join("\n");
            frame.render_widget(
                Paragraph::new(plan).scroll((model.results.explain_scroll, 0)),
                body,
            );
        }
        ResultsView::Messages => {
            frame.render_widget(
                Paragraph::new(message_lines(model))
                    .wrap(Wrap { trim: false })
                    .scroll((model.results.messages_scroll, 0)),
                body,
            );
        }
        ResultsView::Grid => {
            frame.render_widget(Paragraph::new(preview_lines(model, body, hits)), body);
        }
    }
}

/// One row inside the pane holding the view selector and, after a divider, the result
/// sets. The pane's top border is already the drag divider, so nothing can live there.
/// The active entry is bracketed, so it still reads with no color.
fn output_toolbar(model: &Model, hits: &mut HitMap, area: Rect) -> String {
    let mut out = String::new();
    let mut x = area.x;
    let push = |out: &mut String, hits: &mut HitMap, x: &mut u16, text: String, target| {
        let width = text.chars().count() as u16;
        let remaining = area.width.saturating_sub(x.saturating_sub(area.x));
        if remaining > 0 {
            hits.register(target, Rect::new(*x, area.y, width.min(remaining), 1));
        }
        *x = x.saturating_add(width);
        out.push_str(&text);
    };

    for (index, view) in ResultsView::ALL.iter().enumerate() {
        // The count is how you know there is anything in there without switching.
        let label = match view {
            ResultsView::Messages if !model.messages.is_empty() => {
                format!("Messages({})", model.messages.len())
            }
            _ => view.label().to_string(),
        };
        let text = if *view == model.results.view {
            format!("[{label}]")
        } else {
            format!(" {label} ")
        };
        push(&mut out, hits, &mut x, text, HitTarget::ResultsView(index));
    }

    if model.results.view == ResultsView::Explain {
        // cycling the sub-view used to be invisible; the divider keeps it from reading
        // as a fourth view now that Messages sits next to it
        out.push_str(&format!(" │ {:?}", model.explain.view));
    } else if model.results.view == ResultsView::Grid && model.results.tabs.len() > 1 {
        out.push_str(" │");
        x = x.saturating_add(2);
        for (index, tab) in model.results.tabs.iter().enumerate() {
            let text = if index == model.results.active {
                format!("[{}]", tab.title)
            } else {
                format!(" {} ", tab.title)
            };
            push(&mut out, hits, &mut x, text, HitTarget::ResultTab(index));
        }
    }
    out
}

fn result_banner(model: &Model) -> String {
    let mut extra = String::new();
    if let Some(tab) = model.results.tabs.get(model.results.active) {
        // A notice used to be crammed in here, one at a time, truncated by the title.
        // It lives in the Messages view now, with the rest of them.
        if let Some(reason) = &tab.local_only {
            extra.push_str(" local-only:");
            extra.push_str(reason);
        }
    }
    if !model.data.crumbs.is_empty() {
        extra.push_str(" crumbs:");
        extra.push_str(&model.data.crumbs.len().to_string());
    }
    if model.data.page_offset > 0 || model.data.has_more {
        extra.push_str(&format!(
            " page:{}+{}",
            model.data.page_offset, model.data.page_limit
        ));
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
    let body_height = area.height.saturating_sub(HEADER_ROWS) as usize;
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
        let is_pending_delete = matches!(
            model.data.row_changes.get(&row.source_index),
            Some(dexo_app::data::RowEditState::Deleted)
        );
        let row_style = if is_pending_delete {
            model.theme.style(Role::Error, model.capabilities)
        } else if is_active {
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

/// The log, oldest first so the newest is where you land after scrolling down -- and so a
/// burst of messages reads in the order it happened.
fn message_lines(model: &Model) -> Vec<Line<'static>> {
    if model.messages.is_empty() {
        return vec![Line::from(Span::styled(
            "no messages yet",
            model.theme.style(Role::Muted, model.capabilities),
        ))];
    }
    model
        .messages
        .iter()
        .map(|entry| {
            let role = match entry.severity {
                Severity::Info => Role::Muted,
                Severity::Warn => Role::Warning,
                Severity::Error => Role::Error,
            };
            Line::from(vec![
                Span::styled(
                    format!("{:<5} ", entry.severity.label()),
                    model.theme.style(role, model.capabilities),
                ),
                Span::raw(entry.message.clone()),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::model::{GridModel, truncate_cell};
    use crate::render::render_to_string;
    use crate::update::update;

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

    /// The model's row viewport and the rows the widget paints are two derivations of
    /// the same number. They drifted once already, when the toolbar row stopped being
    /// conditional; this pins them to each other rather than to the arithmetic.
    #[test]
    fn the_viewport_holds_exactly_the_rows_the_grid_paints() {
        use crate::layout::LayoutPlan;
        use crate::model::Model;

        let mut model = Model::default();
        *model.results = GridModel::sample_rows(200);
        model.apply_size(120, 40);

        let plan = LayoutPlan::for_area_with_document_tabs(
            ratatui::layout::Rect::new(0, 0, model.width, model.height),
            Some(&model.effective_panes()),
            true,
        );
        let pane = plan.results;
        let inner = ratatui::widgets::Block::bordered().inner(pane);
        let body = Rect::new(
            inner.x,
            inner.y.saturating_add(TOOLBAR_ROWS),
            inner.width,
            inner.height.saturating_sub(TOOLBAR_ROWS),
        );
        let painted = preview_lines(&model, body, &mut HitMap::default()).len() - 1;
        assert_eq!(
            model.results.viewport().height,
            painted,
            "the model believes in rows the pane does not paint"
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

    /// The cursor must always be on a row the pane paints, at every size, in either pane
    /// the grid can occupy, and in any result set. Each of these has drifted on its own.
    #[test]
    fn the_cursor_is_painted_at_every_size_and_in_every_result_set() {
        use crate::model::{EditorDocument, GridModel, ResultTab};

        for (w, h) in [(100u16, 24u16), (160, 50), (80, 24), (120, 35), (200, 60)] {
            for table in [false, true] {
                let mut model = Model::default();
                if table {
                    model
                        .documents
                        .push(EditorDocument::new_table(dexo_app::parse_qualified(
                            "public.orders",
                        )));
                    model.active_document = 1;
                }
                *model.results = GridModel::sample_rows(500);
                model.apply_size(w, h);

                let key = model.results.tabs[0].key.clone();
                let mut second = ResultTab::new(key, "result 2");
                second.grid = GridModel::sample_rows(500);
                model.results.push_tab(second);

                for index in [0, 1] {
                    update(&mut model, Action::SelectResultTab { index });
                    for step in 0..30 {
                        update(&mut model, Action::ResultsDown);
                        if step % 7 == 0 {
                            update(&mut model, Action::ResultsPageDown);
                        }
                        let cursor = model.results.cursor_row().expect("cursor");
                        let view = render_to_string(&model, w, h);
                        assert!(
                            view.contains(&format!("▸ {cursor} ")),
                            "{w}x{h} table={table} set={index}: \
                             the cursor is on row {cursor}, which the pane does not paint"
                        );
                    }
                }
            }
        }
    }

    /// Every result tab is drawn in the same pane, so every one has to be sized by it.
    /// Only the active tab ever was: any other kept `GridViewport::default()`'s twenty
    /// rows, and walking down it moved the cursor past what the pane paints without ever
    /// scrolling -- the selection simply stopped being on screen.
    #[test]
    fn every_result_tab_is_sized_by_the_pane_that_draws_it() {
        use crate::model::{EditorDocument, GridModel, ResultTab};

        // a table document, whose grid height actually tracks the terminal
        let mut model = Model::default();
        model
            .documents
            .push(EditorDocument::new_table(dexo_app::parse_qualified(
                "public.orders",
            )));
        model.active_document = 1;
        *model.results = GridModel::sample_rows(500);
        model.apply_size(100, 24);

        let key = model.results.tabs[0].key.clone();
        let mut second = ResultTab::new(key, "result 2");
        second.grid = GridModel::sample_rows(500);
        model.results.push_tab(second);
        // resize after the tab exists: the pane has to reach every tab, not just the
        // one that happens to be on screen when it changes size
        model.apply_size(120, 35);
        let painted = model.results.viewport().height;
        assert!(painted > 20, "pick a size where the two differ: {painted}");
        update(&mut model, Action::SelectResultTab { index: 1 });

        assert_eq!(
            model.results.viewport().height,
            painted,
            "the second result set is sized by something other than its pane"
        );

        for _ in 0..(painted + 4) {
            update(&mut model, Action::ResultsDown);
        }
        let cursor = model.results.cursor_row().expect("cursor");
        let view = render_to_string(&model, 120, 35);
        assert!(
            view.contains(&format!("▸ {cursor} ")),
            "the cursor walked off the pane at row {cursor}:\n{view}"
        );
    }

    /// A table document has no editor: the grid takes that slot and the bottom pane is
    /// just a log. Sharing the editor's split left the grid -- the entire screen there --
    /// with four rows out of twenty-four.
    #[test]
    fn a_table_document_gives_the_grid_the_room_the_editor_would_have_had() {
        use crate::model::EditorDocument;

        let mut model = Model::default();
        model
            .documents
            .push(EditorDocument::new_table(dexo_app::parse_qualified(
                "public.orders",
            )));
        *model.results = crate::model::GridModel::sample_rows(200);

        model.active_document = 0;
        model.apply_size(100, 24);
        let in_editor = model.results.viewport().height;

        model.active_document = 1;
        model.apply_size(100, 24);
        let on_the_table = model.results.viewport().height;

        assert!(
            on_the_table > in_editor,
            "the grid is still sharing the editor's split: {on_the_table} vs {in_editor}"
        );
        // it owns the screen there, so it should hold at least half the terminal
        assert!(
            on_the_table >= 12,
            "the grid is cramped on the one screen it owns: {on_the_table} rows of 24"
        );
        // and the console is still there, just out of the way
        assert!(render_to_string(&model, 100, 24).contains("Console"));
    }

    /// The pane draws a toolbar row that the viewport arithmetic did not subtract, so
    /// the model believed in one row more than the grid shows. Walking to the bottom
    /// then parked the cursor on a row drawn past the border -- the selection vanished.
    #[test]
    fn the_cursor_row_stays_on_screen_at_the_bottom_of_the_grid() {
        use crate::model::EditorDocument;

        let mut model = Model::default();
        model
            .documents
            .push(EditorDocument::new_table(dexo_app::parse_qualified(
                "public.orders",
            )));
        model.active_document = 1;
        *model.results = crate::model::GridModel::sample_rows(200);
        model.apply_size(120, 40);

        for _ in 0..199 {
            update(&mut model, Action::ResultsDown);
        }
        assert_eq!(model.results.cursor_row(), Some(199));

        let view = render_to_string(&model, 120, 40);
        assert!(
            view.contains("199"),
            "the row under the cursor is drawn past the pane:\n{view}"
        );
    }

    /// Until this view existed a message got one toast and was then unreachable: the log
    /// was in the model and rendered nowhere.
    #[test]
    fn the_messages_view_shows_the_log_with_its_severities() {
        let mut model = Model {
            focus: Focus::Results,
            ..Model::default()
        };
        model.results.view = ResultsView::Messages;

        assert!(
            render_to_string(&model, 120, 40).contains("no messages yet"),
            "an empty log needs to say so"
        );

        model.messages.info("saved staging".into());
        model.messages.warn("connection is read-only".into());
        model
            .messages
            .error("relation \"orders\" does not exist".into());

        let view = render_to_string(&model, 120, 40);
        for text in [
            "saved staging",
            "connection is read-only",
            "does not exist",
            "Messages(3)",
        ] {
            assert!(view.contains(text), "missing {text}: {view}");
        }
        // the severity is a word, so it survives with no color at all
        assert!(view.contains("info "), "{view}");
        assert!(view.contains("warn "), "{view}");
        assert!(view.contains("error"), "{view}");
    }

    /// The log is the one list in the pane that only grows, so it scrolls on its own
    /// axis rather than moving the grid cursor.
    #[test]
    fn messages_scroll_without_touching_the_grid_cursor() {
        let mut model = Model::default();
        model.results.view = ResultsView::Messages;
        for index in 0..5 {
            model.messages.info(format!("entry {index}"));
        }
        let row = model.results.cursor_row();

        update(&mut model, Action::ResultsDown);
        update(&mut model, Action::ResultsDown);
        assert_eq!(model.results.messages_scroll, 2);
        assert_eq!(model.results.cursor_row(), row, "the grid cursor moved");

        update(&mut model, Action::ResultsUp);
        assert_eq!(model.results.messages_scroll, 1);
    }
}
