//! Compare: two schemas side by side as differences, the statement for the picked one or
//! the whole migration script beside them. It was Compare Schema, one dialog of
//! eighty-eight columns that held the sources, the list and the script in turn.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use crate::model::Model;
use crate::mouse::{HitButton, HitMap, HitTarget};

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let diff = &model.schema_diff;
    if diff.source_prompt {
        sources(frame, area, model, hits);
        return;
    }
    // The comparison in a line, and its filters, each a click.
    let (body, _, _) = diff.body(usize::from(area.width));
    for (index, line) in body.iter().take(2).enumerate() {
        let row = Rect::new(area.x, area.y + index as u16, area.width, 1);
        frame.render_widget(Paragraph::new(line.clone()), row);
        if line.starts_with("Show:") {
            for (needle, button) in [
                ("[x] added", HitButton::ToggleAdded),
                ("[ ] added", HitButton::ToggleAdded),
                ("[x] removed", HitButton::ToggleRemoved),
                ("[ ] removed", HitButton::ToggleRemoved),
                ("[x] changed", HitButton::ToggleChanged),
                ("[ ] changed", HitButton::ToggleChanged),
            ] {
                crate::mouse::register_label(hits, row, line, needle, HitTarget::Button(button));
            }
        }
    }
    let rest = Rect::new(
        area.x,
        area.y + 2.min(area.height),
        area.width,
        area.height.saturating_sub(2),
    );
    let shown = diff.filtered();
    if shown.is_empty() {
        let mut lines = vec![if diff.entries.is_empty() {
            "The two schemas are the same.".to_string()
        } else {
            "Every difference is filtered out; a, r and c bring them back.".to_string()
        }];
        lines.extend(diff.error.clone());
        super::empty_state(frame, rest, model, &lines);
        return;
    }
    let rows: Vec<String> = shown
        .iter()
        .map(|entry| {
            if entry.risk.is_empty() {
                format!("{:<8} {}", entry.kind, entry.object)
            } else {
                format!("{:<8} {}  ({})", entry.kind, entry.object, entry.risk)
            }
        })
        .collect();
    let (list, detail) = super::list_and_detail(rest, rows.len());
    super::list_pane(
        frame,
        list,
        model,
        hits,
        &format!("Differences ({})", rows.len()),
        None,
        &rows,
        Some(diff.selected),
    );
    let title = if diff.whole_script {
        "Migration script"
    } else {
        "Statement"
    };
    let (_, max_scroll, page) = super::text_pane(
        frame,
        detail,
        model,
        hits,
        title,
        &diff.detail_lines(),
        usize::from(diff.scroll),
        &diff.error.clone().into_iter().collect::<Vec<_>>(),
    );
    hits.set_scroll_limit(crate::mouse::ScrollArea::SchemaDiff, max_scroll);
    hits.set_page(crate::mouse::ScrollArea::SchemaDiff, page);
}

/// The two sources to compare, at the top of the screen.
fn sources(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let diff = &model.schema_diff;
    let width = area.width.min(100);
    let (mut lines, _, _) = diff.body(usize::from(width.saturating_sub(2)));
    let footer_row = lines.len();
    let footer = crate::widgets::form::footer_line(diff.submit_label(), diff.footer);
    lines.push(footer.clone());
    let height = (lines.len() as u16 + 2).min(area.height);
    let pane = Rect::new(area.x, area.y, width, height);
    let block = crate::render::pane_block(model, "Sources", true);
    let inner = block.inner(pane);
    frame.render_widget(block, pane);
    frame.render_widget(Paragraph::new(lines.join("\n")), inner);
    for index in 0..lines.len().min(usize::from(inner.height)) {
        let rect = crate::mouse::line_rect(inner, index);
        if index == footer_row {
            crate::widgets::form::register_footer(hits, rect, &footer, diff.submit_label());
        } else if (2..=4).contains(&index) {
            hits.register(HitTarget::FormField(index - 2), rect);
            if index == 4 && diff.uses_file() && diff.row == 2 {
                crate::render::show_input(frame, rect, "> File: ", &diff.file, false);
            }
        }
    }
}

pub fn hints(model: &Model) -> String {
    let diff = &model.schema_diff;
    if diff.source_prompt {
        "Up/Down row  Left/Right change a source  Enter compare  Esc back".into()
    } else if diff.whole_script {
        "w the picked one  PgUp/PgDn read  Enter open the script  e new comparison  Esc back".into()
    } else {
        "a/r/c filter  Up/Down pick  w whole script  Enter open the script  e new comparison  Esc back"
            .into()
    }
}
