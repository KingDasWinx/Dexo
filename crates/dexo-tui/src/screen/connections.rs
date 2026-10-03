//! Connections: every saved connection beside what it points at, the databases found in
//! Docker, and the Add and Edit form in place of the details. It was a dialog of
//! seventy-two columns whose form scrolled through twenty-five fields.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::model::Model;
use crate::mouse::{HitButton, HitMap, HitTarget};

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let screen = &model.connections;
    let rows = screen.rows(model.active_session);
    if screen.profiles.is_empty() && screen.unsaved_docker().next().is_none() {
        if model.connection_form.open {
            form(frame, area, model, hits);
            return;
        }
        let mut lines = vec![
            "No connections yet.".to_string(),
            "n adds one; r looks for databases running in Docker.".to_string(),
        ];
        lines.extend(screen.error.clone());
        super::empty_state(frame, area, model, &lines);
        return;
    }
    let (list, detail) = super::list_and_detail(area, rows.len());
    list_pane(frame, list, model, hits, &rows);
    if model.connection_form.open {
        form(frame, detail, model, hits);
    } else {
        let width = usize::from(detail.width.saturating_sub(2)).max(8);
        let lines: Vec<String> = screen
            .detail_lines(model.active_session)
            .iter()
            .flat_map(|line| crate::model::wrap_words(line, width))
            .collect();
        let title = screen
            .selected()
            .map(|profile| profile.name.clone())
            .or_else(|| {
                screen
                    .selected_docker()
                    .map(|database| database.container.clone())
            })
            .unwrap_or_default();
        // What can be done to the pick, each one a click, under its details.
        let actions = screen.footer_lines(usize::from(detail.width.saturating_sub(2)));
        let (footer, _, _) =
            super::text_pane(frame, detail, model, hits, &title, &lines, 0, &actions);
        let buttons = [
            HitButton::Connect,
            HitButton::New,
            HitButton::Edit,
            HitButton::Duplicate,
            HitButton::Test,
            HitButton::Delete,
            HitButton::CloseSession,
            HitButton::Docker,
        ];
        for (index, line) in actions.iter().enumerate().take(usize::from(footer.height)) {
            let rect = crate::mouse::line_rect(footer, index);
            for (label, button) in crate::screens::connections::HINTS.iter().zip(buttons) {
                crate::mouse::register_label(hits, rect, line, label, HitTarget::Button(button));
            }
        }
    }
}

/// The saved connections, then the Docker ones under their heading, the pick kept in
/// sight; a heading is text, not a row to pick.
fn list_pane(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    rows: &[(Option<usize>, String)],
) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let screen = &model.connections;
    let focused = !model.connection_form.open;
    let block = crate::render::pane_block(
        model,
        &format!("Connections ({})", screen.profiles.len()),
        focused,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let visible = usize::from(inner.height);
    let picked = rows
        .iter()
        .position(|(row, _)| *row == Some(screen.selected_profile))
        .unwrap_or(0);
    let offset = crate::palette::scroll_to_selection(picked, 0, rows.len(), visible);
    let width = usize::from(inner.width);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible)
        .map(|(line, (_, text))| {
            let text = crate::model::truncate_cell(text, width);
            if line == picked {
                Line::styled(
                    format!("{text:<width$}"),
                    Style::default().add_modifier(Modifier::REVERSED),
                )
            } else {
                Line::raw(text)
            }
        })
        .collect();
    for (line, (row, _)) in rows.iter().enumerate().skip(offset).take(visible) {
        if let Some(row) = row {
            hits.register(
                HitTarget::ListRow(*row),
                crate::mouse::line_rect(inner, line - offset),
            );
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Add or Edit connection, in the pane the details were in.
fn form(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let form = &model.connection_form;
    let block = crate::render::pane_block(model, form.title(), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }
    let visible = form.visible_rows(usize::from(inner.height).max(4), usize::from(inner.width));
    let lines: Vec<String> = visible.iter().map(|(_, line)| line.clone()).collect();
    frame.render_widget(Paragraph::new(lines.join("\n")), inner);
    let footer = lines.len().saturating_sub(1);
    for (index, line) in lines.iter().enumerate().take(usize::from(inner.height)) {
        let rect = crate::mouse::line_rect(inner, index);
        if index == footer {
            crate::widgets::form::register_footer(hits, rect, line, "Submit");
            crate::mouse::register_label(
                hits,
                rect,
                line,
                "[Test]",
                HitTarget::Button(HitButton::Test),
            );
            continue;
        }
        // The status rows above the buttons are text, not fields.
        let Some(field) = visible.get(index).and_then(|(field, _)| *field) else {
            continue;
        };
        if line.contains("Advanced options") {
            hits.register(HitTarget::Button(HitButton::ToggleAdvanced), rect);
            continue;
        }
        if field == form.focus
            && !form.is_choice_at(field)
            && let Some(value) = form.fields.get(field)
        {
            crate::render::show_form_field(frame, rect, value);
        }
        hits.register(HitTarget::FormField(field), rect);
        if form.is_choice_at(field) {
            for (needle, step) in [("< ", -1), (" >", 1)] {
                crate::mouse::register_label(
                    hits,
                    rect,
                    line,
                    needle,
                    HitTarget::FormChoice { index: field, step },
                );
            }
        }
    }
}

pub fn hints(model: &Model) -> String {
    if model.connection_form.open {
        "Tab next  Left/Right pick a value  Enter save  Esc cancel".into()
    } else {
        "Up/Down pick  Esc back".into()
    }
}
