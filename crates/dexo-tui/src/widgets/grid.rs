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
/// bottom, where the selection is invisible. The WHERE / ORDER BY row is one more when
/// the grid shows it.
pub fn chrome_rows(model: &Model) -> u16 {
    TOOLBAR_ROWS + HEADER_ROWS + u16::from(bars_row(model))
}

const TOOLBAR_ROWS: u16 = 1;
const HEADER_ROWS: u16 = 1;

/// The WHERE / ORDER BY row is over the grid of anything that can run again.
fn bars_row(model: &Model) -> bool {
    model.results.view == ResultsView::Grid && crate::update::clause_bars_shown(model)
}

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if area.width < 2 || area.height < 2 {
        frame.render_widget(Paragraph::new(preview_lines(model, area, hits)), area);
        return;
    }
    let extra = result_banner(model);
    // Nothing has run into the pane: no count to give.
    let title = if model.results.columns().is_empty() {
        format!("Results{extra}")
    } else if model.results.truncated() {
        format!("Results ({}) …{extra}", rows_label(model))
    } else {
        format!("Results ({}){extra}", rows_label(model))
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
            let styles = crate::screens::explain::ExplainStyles {
                muted: model.theme.style(Role::Muted, model.capabilities),
                warning: model.theme.style(Role::Warning, model.capabilities),
                unicode: model.capabilities.unicode,
            };
            let plan = model.results.explain.lines(body.width, &styles);
            let max_scroll = plan.len().saturating_sub((body.height as usize).max(1));
            hits.set_scroll_limit(crate::mouse::ScrollArea::Explain, max_scroll);
            let scroll = (model.results.explain_scroll as usize).min(max_scroll) as u16;
            frame.render_widget(Paragraph::new(plan).scroll((scroll, 0)), body);
        }
        ResultsView::Messages => {
            frame.render_widget(
                Paragraph::new(message_lines(model))
                    .wrap(Wrap { trim: false })
                    .scroll((model.results.messages_scroll, 0)),
                body,
            );
        }
        ResultsView::Grid if model.expanded_records && model.results.row_count() > 0 => {
            frame.render_widget(Paragraph::new(record_lines(model, body)), body);
        }
        ResultsView::Grid => {
            let body = if bars_row(model) {
                render_clause_bars(frame, Rect { height: 1, ..body }, model, hits);
                Rect {
                    y: body.y + 1,
                    height: body.height.saturating_sub(1),
                    ..body
                }
            } else {
                body
            };
            frame.render_widget(Paragraph::new(preview_lines(model, body, hits)), body);
        }
    }
}

/// `WHERE [...]  ORDER BY [...]`: the text typed, or what the key is when there is none,
/// and the terminal cursor in the bar that has the keys. Each bar is a click target.
fn render_clause_bars(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    use crate::screens::data::ClauseBar;
    let bars = &model.data.bars;
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let focus = crate::update::focused_bar(model);
    let label = |bar: ClauseBar| {
        if focus == Some(bar) {
            model
                .theme
                .style(Role::Focus, model.capabilities)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            muted
        }
    };
    const WHERE: &str = "WHERE ";
    const ORDER: &str = " ORDER BY ";
    let where_field = (area.width as usize * 3 / 5)
        .max(16)
        .saturating_sub(WHERE.len());
    let order_start = WHERE.len() + where_field + ORDER.len();
    let order_field = (area.width as usize).saturating_sub(order_start);
    let field = |input: &crate::widgets::text_input::TextInput, hint: &str, width: usize| {
        if input.is_empty() {
            return (
                ratatui::text::Span::styled(
                    format!("{:width$}", truncate_cell(hint, width)),
                    muted,
                ),
                0,
            );
        }
        let (shown, cursor) = input.window(width);
        (ratatui::text::Span::raw(shown), cursor)
    };
    let (where_span, where_cursor) = field(&bars.where_input, "w to filter", where_field);
    let (order_span, order_cursor) = field(&bars.order_input, "o to sort", order_field);
    let line = ratatui::text::Line::from(vec![
        ratatui::text::Span::styled(WHERE, label(ClauseBar::Where)),
        where_span,
        ratatui::text::Span::styled(ORDER, label(ClauseBar::Order)),
        order_span,
    ]);
    frame.render_widget(Paragraph::new(line), area);
    let span = |start: usize, width: usize| {
        let start = (start as u16).min(area.width);
        Rect::new(
            area.x + start,
            area.y,
            (width as u16).min(area.width - start),
            1,
        )
    };
    hits.register(
        HitTarget::ClauseBar(ClauseBar::Where),
        span(0, WHERE.len() + where_field),
    );
    hits.register(
        HitTarget::ClauseBar(ClauseBar::Order),
        span(WHERE.len() + where_field, ORDER.len() + order_field),
    );
    if let Some(bar) = focus {
        let x = match bar {
            ClauseBar::Where => WHERE.len() + where_cursor,
            ClauseBar::Order => order_start + order_cursor,
        };
        if x < area.width as usize {
            frame.set_cursor_position(ratatui::layout::Position::new(area.x + x as u16, area.y));
        }
    }
}

/// `\\x`: from the cursor's row on, each row as psql's expanded display shows it -- a
/// `-[ RECORD n ]-` rule, then one field per line -- as many as fit.
fn record_lines(model: &Model, area: Rect) -> Vec<ratatui::text::Line<'static>> {
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let first = model.results.cursor_row().unwrap_or(0);
    let mut lines = Vec::new();
    for row in first..model.results.row_count() {
        if lines.len() >= area.height as usize {
            break;
        }
        let rule = format!("-[ RECORD {} ]", row + 1);
        let fill = (area.width as usize).saturating_sub(rule.chars().count());
        lines.push(ratatui::text::Line::styled(
            format!("{rule}{}", "-".repeat(fill)),
            muted,
        ));
        let fields = crate::widgets::row_detail::row_detail_fields(&model.results, row);
        let width = fields
            .iter()
            .map(|field| field.name.chars().count())
            .max()
            .unwrap_or(0);
        for field in fields {
            let (crate::widgets::row_detail::RowDetailValue::Text(value)
            | crate::widgets::row_detail::RowDetailValue::Json(value)) = field.value;
            // One line a field, as psql prints it; the value's own breaks would push
            // the next field off the screen, so they show as spaces.
            let value = value.replace(['\n', '\r'], " ");
            lines.push(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(format!("{:<width$} │ ", field.name), muted),
                ratatui::text::Span::raw(value),
            ]));
        }
    }
    lines
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
        out.push_str(&format!(" │ {:?}", model.results.explain.view));
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

/// How many rows there are, and how sure that is: `843 rows` when every row is in, `~4.3M
/// rows` from the server's statistics, `300+ rows` when only a floor is known, and
/// `10,000+ rows, limit reached` when a statement's rows stopped at the limit. A count
/// asked for with `t` replaces all of them while the grid still shows what it counted.
fn rows_label(model: &Model) -> String {
    use crate::screens::data::CountState;
    let counted = model
        .data
        .count
        .as_ref()
        .filter(|count| crate::update::count_key(model).as_ref() == Some(&count.key));
    if let Some(CountState::Exact(rows)) = counted.map(|count| count.state) {
        return rows_of(&grouped(rows), rows);
    }
    let shown = model.results.row_count() as u64;
    let label = if model.active_document().kind.is_table() {
        let seen = model.data.page_offset + shown;
        match (model.data.has_more, model.data.estimated_total) {
            (false, _) => rows_of(&grouped(seen), seen),
            (true, Some(total)) if total > seen => format!("~{} rows", compact(total)),
            (true, _) => format!("{}+ rows", grouped(seen)),
        }
    } else if model
        .results
        .tabs
        .get(model.results.active)
        .is_some_and(|tab| tab.paged)
    {
        // One page of the result: a full one may have more after it.
        let seen = model.data.page_offset + shown;
        if shown >= u64::from(model.data.page_limit) {
            format!("{}+ rows", grouped(seen))
        } else {
            rows_of(&grouped(seen), seen)
        }
    } else if model
        .results
        .tabs
        .get(model.results.active)
        .is_some_and(|tab| tab.truncated)
    {
        format!("{}+ rows, limit reached", grouped(shown))
    } else {
        rows_of(&grouped(shown), shown)
    };
    if counted.is_some() {
        let dots = if model.capabilities.unicode {
            "…"
        } else {
            "..."
        };
        format!("{label}, counting{dots}")
    } else {
        label
    }
}

/// `1 row`, `12 rows`.
fn rows_of(number: &str, count: u64) -> String {
    if count == 1 {
        format!("{number} row")
    } else {
        format!("{number} rows")
    }
}

/// `10000` as `10,000`.
fn grouped(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// An estimate as it reads best: `843`, `12.4K`, `4.3M`, `1.2B`. A number that rounds
/// up to the next unit is written in it: 999,950 is `1.0M`, not `1000.0K`.
fn compact(number: u64) -> String {
    if number < 1_000 {
        return number.to_string();
    }
    let mut value = number as f64;
    for unit in ["K", "M", "B"] {
        value /= 1_000.0;
        if (value * 10.0).round() < 10_000.0 || unit == "B" {
            return format!("{value:.1}{unit}");
        }
    }
    unreachable!("B is the last unit")
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
    // The rows a foreign key led to: the WHERE bar does not hold this filter, so the
    // title says it.
    if model.active_document().kind.is_table()
        && let Some(filter) = &model.data.filter
    {
        extra.push_str(" where ");
        extra.push_str(&crate::screens::data_browser::describe_filter(filter));
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

/// Each sorted column's marker -- `▲1`, `▼2`, or `^1`, `v2` without Unicode -- read from
/// the ORDER BY that ran. Text no header can show marks nothing.
fn sort_markers(model: &Model) -> impl Fn(&str) -> Option<String> {
    let keys = model
        .data
        .bars
        .applied
        .order_by
        .as_deref()
        .filter(|_| crate::update::clause_bars_shown(model))
        .and_then(|text| dexo_sql::order_keys(text, crate::screens::editor::editor_dialect(model)))
        .unwrap_or_default();
    let (up, down) = if model.capabilities.unicode {
        ("▲", "▼")
    } else {
        ("^", "v")
    };
    move |name: &str| {
        let at = keys.iter().position(|key| key.names(name))?;
        let arrow = if keys[at].descending { down } else { up };
        Some(format!("{arrow}{}", at + 1))
    }
}

fn preview_lines(model: &Model, area: Rect, hits: &mut HitMap) -> Vec<Line<'static>> {
    let grid = &model.results;
    let col_indices = grid.visible_column_indices();
    let widths = grid.column_widths();
    let sorted = sort_markers(model);
    // A sorted column is as wide as its name and its marker.
    let natural_widths: Vec<u16> = col_indices
        .iter()
        .map(|&index| {
            let width = widths.get(index).copied().unwrap_or(8);
            let marked = grid.columns().get(index).and_then(|column| {
                let marker = sorted(&column.name)?;
                let wide = unicode_width::UnicodeWidthStr::width;
                u16::try_from(wide(column.name.as_str()) + 1 + wide(marker.as_str())).ok()
            });
            width.max(marked.unwrap_or(0))
        })
        .collect();
    let (cell_widths, overflowed) = allocate_column_widths(&natural_widths, area.width as usize);
    let mut header = Vec::new();
    let mut remaining = area.width as usize;
    let header_style = model.theme.header(model.capabilities);
    let current_column = grid.selection().map(|(_, col)| col);
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
        // The marker stays whole; the name gives way to it.
        let label = match sorted(&column.name) {
            Some(marker) => {
                let room = cell_width
                    .saturating_sub(unicode_width::UnicodeWidthStr::width(marker.as_str()) + 1);
                format!("{} {marker}", truncate_cell(&column.name, room))
            }
            None => column.name.clone(),
        };
        // The current column -- the one `s` sorts -- is marked without colour too.
        let style = if current_column == Some(index) {
            header_style.add_modifier(ratatui::style::Modifier::REVERSED)
        } else {
            header_style
        };
        header.push(Span::styled(
            format!(
                "{:width$}",
                truncate_cell(&label, cell_width),
                width = cell_width
            ),
            style,
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
            let marker = sel_marker.chars().count() + 1;
            spans.push(Span::styled(format!("{sel_marker} "), row_style));
            remaining = remaining.saturating_sub(marker);
            // The marker takes the first column's breathing room instead of pushing the
            // row sideways: re-fitting the widths here left every column on the cursor
            // row two characters off from the header above it.
            let mut widths = cell_widths.clone();
            if let Some(first) = widths.first_mut() {
                *first = first.saturating_sub(marker as u16).max(1);
            }
            (widths, overflowed)
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
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let mut lines = Vec::new();
    for entry in model.messages.iter() {
        let role = match entry.severity {
            Severity::Info => Role::Muted,
            Severity::Warn => Role::Warning,
            Severity::Error => Role::Error,
        };
        let stamp = format!("[{}] ", entry.at);
        let indent = " ".repeat(stamp.chars().count() + 6);
        lines.push(Line::from(vec![
            Span::styled(stamp, muted),
            Span::styled(
                format!("{:<5} ", entry.severity.label()),
                model.theme.style(role, model.capabilities),
            ),
            Span::raw(entry.message.clone()),
        ]));
        // Under the message and aligned with it, dimmer, so the eye lands on the message
        // first and the SQLSTATE, caret, DETAIL and HINT read as its footnotes.
        for detail in &entry.details {
            lines.push(Line::from(Span::styled(format!("{indent}{detail}"), muted)));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    /// Estimates roll over to the next unit when they round up to it, and one row is a
    /// row.
    #[test]
    fn labels_read_right_at_their_edges() {
        assert_eq!(super::compact(999), "999");
        assert_eq!(super::compact(12_400), "12.4K");
        assert_eq!(super::compact(999_949), "999.9K");
        assert_eq!(super::compact(999_950), "1.0M");
        assert_eq!(super::compact(999_999_999), "1.0B");
        assert_eq!(super::rows_of("1", 1), "1 row");
        assert_eq!(super::rows_of("2", 2), "2 rows");
    }

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
        let pad = crate::model::COLUMN_PADDING;
        grid.set_viewport_size(40, 10);
        assert_eq!(
            grid.column_widths()[0],
            2 + pad,
            "first rows only need two digits"
        );

        grid.scroll_rows(120);
        assert_eq!(
            grid.column_widths()[0],
            3 + pad,
            "three-digit ids must not be clipped once they scroll into view"
        );

        grid.scroll_rows(-120);
        assert_eq!(
            grid.column_widths()[0],
            3 + pad,
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

    /// Walking down must land on the last row the pane paints -- if a row is painted
    /// below the cursor and the cursor cannot reach it, the pane is lying about how far
    /// the list goes. `sync_grid_viewport` picked its pane by document kind, but Compact
    /// has one pane and what fills it is decided by focus, so an editor document there
    /// was sized against a zero-height results rect: the cursor stopped on the first row
    /// with a full screen of rows painted below it.
    #[test]
    fn the_cursor_reaches_the_last_row_the_pane_paints() {
        use crate::model::EditorDocument;

        for w in [60u16, 80, 100, 120, 160, 267] {
            for h in [20u16, 24, 30, 40, 59] {
                for table in [false, true] {
                    let mut model = Model::default();
                    if table {
                        model.documents.push(EditorDocument::new_table(
                            dexo_app::parse_qualified("public.orders"),
                            None,
                        ));
                        model.set_active_document(1);
                    }
                    *model.results = GridModel::sample_rows(500);
                    model.focus = Focus::Results;
                    model.apply_size(w, h);

                    for _ in 0..200 {
                        update(&mut model, Action::ResultsDown);
                    }
                    let cursor = model.results.cursor_row().expect("cursor");
                    let view = render_to_string(&model, w, h);
                    assert!(
                        view.contains(&format!("▸ {cursor} ")),
                        "{w}x{h} table={table}: the cursor is not painted at all"
                    );
                    assert!(
                        !view.contains(&format!("│{} ", cursor + 1)),
                        "{w}x{h} table={table}: row {} is painted below a cursor that \
                         stopped at {cursor}",
                        cursor + 1
                    );
                }
            }
        }
    }

    /// Opening a table document moves the grid from the bottom pane into the editor's
    /// slot and leaves the bottom pane holding only the console. The viewport is derived
    /// from that pane, but nothing re-derived it when the active document changed: the
    /// grid kept the handful of rows the editor's results pane had, so the cursor
    /// scrolled the list under itself a couple of rows down with a full pane painted
    /// below it.
    #[test]
    fn switching_to_a_table_document_resizes_the_row_viewport() {
        use crate::action::Action;
        use crate::model::EditorDocument;

        for (w, h) in [(160u16, 50u16), (267, 59), (120, 35), (100, 30)] {
            let mut model = Model::default();
            model.documents.push(EditorDocument::new_table(
                dexo_app::parse_qualified("public.orders"),
                None,
            ));
            // Rows in the table tab's own pane, which no resize has sized yet: results
            // are per document, so only the switch can derive its viewport.
            *model.documents[1].results = GridModel::sample_rows(500);
            model.apply_size(w, h);
            update(&mut model, Action::SelectDocument { index: 1 });

            for _ in 0..200 {
                update(&mut model, Action::ResultsDown);
            }
            let cursor = model.results.cursor_row().expect("cursor");
            let view = render_to_string(&model, w, h);
            assert!(
                view.contains(&format!("\u{25b8} {cursor} ")),
                "{w}x{h}: the cursor is not painted at all"
            );
            assert!(
                !view.contains(&format!("\u{2502}{} ", cursor + 1)),
                "{w}x{h}: row {} is painted below a cursor that stopped at {cursor}",
                cursor + 1
            );
        }
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
                    model.documents.push(EditorDocument::new_table(
                        dexo_app::parse_qualified("public.orders"),
                        None,
                    ));
                    model.set_active_document(1);
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
        model.documents.push(EditorDocument::new_table(
            dexo_app::parse_qualified("public.orders"),
            None,
        ));
        model.set_active_document(1);
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
        model.documents.push(EditorDocument::new_table(
            dexo_app::parse_qualified("public.orders"),
            None,
        ));
        *model.results = crate::model::GridModel::sample_rows(200);

        model.set_active_document(0);
        model.apply_size(100, 24);
        let in_editor = model.results.viewport().height;

        model.set_active_document(1);
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
        model.documents.push(EditorDocument::new_table(
            dexo_app::parse_qualified("public.orders"),
            None,
        ));
        model.set_active_document(1);
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

    /// Columns are sized to their widest value plus `COLUMN_PADDING`, and the selection
    /// marker spends the first column's share of it -- re-fitting the widths for the
    /// marker used to slide every column on the cursor row two characters right of the
    /// header above it.
    #[test]
    fn the_cursor_row_lines_up_with_the_header() {
        use crate::model::{COLUMN_PADDING, Model};
        use dexo_driver_api::{ColumnMeta, DbValue};

        let mut model = Model::default();
        model.results.set_columns(
            ["id", "name"]
                .iter()
                .map(|name| ColumnMeta {
                    name: (*name).into(),
                    type_name: "text".into(),
                    nullable: true,
                })
                .collect(),
        );
        model.results.append_rows(
            (2..5)
                .map(|i| vec![DbValue::I64(i), DbValue::Text(format!("Marca {i}"))])
                .collect(),
        );
        model.apply_size(120, 24);
        model.results.select_row(1);

        assert_eq!(
            model.results.column_widths()[0],
            "id".len() as u16 + COLUMN_PADDING,
            "a column no wider than its header must still carry its breathing room"
        );
        let view = render_to_string(&model, 120, 24);
        // Counted in characters: the borders around the pane are multi-byte, and so is
        // the marker on the cursor row.
        let column_of = |needle: &str| {
            view.lines()
                .find_map(|line| line.find(needle).map(|at| line[..at].chars().count()))
                .unwrap_or_else(|| panic!("{needle} is not on screen:\n{view}"))
        };
        assert_eq!(
            column_of("Marca 3"),
            column_of("Marca 2"),
            "the cursor row does not line up with the rows around it:\n{view}"
        );
        assert_eq!(
            column_of("Marca 2"),
            column_of("name"),
            "the rows do not line up with the header:\n{view}"
        );
    }
}
