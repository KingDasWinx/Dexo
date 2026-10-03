//! Connections: every saved connection beside what it points at, the databases found in
//! Docker, and the Add and Edit form in place of the details. It was a dialog of
//! seventy-two columns whose form scrolled through twenty-five fields.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crossterm::event::KeyCode;

use super::Button;
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
    // On a narrow screen the form takes it whole: under the list it scrolled twenty
    // fields through five rows.
    if model.connection_form.open && area.width < 100 {
        form(frame, area, model, hits);
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
        // What can be done to the pick is buttons over its details.
        let footer: Vec<String> = screen.error.clone().into_iter().collect();
        super::detail_pane(
            frame,
            detail,
            model,
            hits,
            &title,
            &buttons(model),
            &lines,
            usize::from(super::detail_scroll(model)),
            &footer,
        );
    }
}

/// What can be done to the picked connection: connect or use it, close its session, edit,
/// duplicate, test or delete it; a database found in Docker is added as a connection.
pub fn buttons(model: &Model) -> Vec<Button> {
    let screen = &model.connections;
    if screen.selected_docker().is_some() {
        return vec![Button::new(KeyCode::Enter, "Add as connection")];
    }
    let Some(profile) = screen.selected() else {
        return Vec::new();
    };
    let session = screen.session_for(&profile.name);
    let in_use = session.is_some_and(|session| model.active_session == Some(session.id));
    let open = match (session.is_some(), in_use) {
        (false, _) => Button::new(KeyCode::Enter, "Connect"),
        (true, false) => Button::new(KeyCode::Enter, "Use"),
        (true, true) => Button::new(KeyCode::Enter, "Connect")
            .disabled(format!("{} is the connection in use.", profile.name)),
    };
    vec![
        open,
        Button::new(KeyCode::Char('c'), "Disconnect").enabled_if(
            session.is_some(),
            format!("{} is not connected.", profile.name),
        ),
        Button::new(KeyCode::Char('e'), "Edit"),
        Button::new(KeyCode::Char('d'), "Duplicate"),
        Button::new(KeyCode::Char('t'), "Test"),
        Button::new(KeyCode::Char('x'), "Delete"),
    ]
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
    let focused = super::section(model) == super::Section::List;
    let block = crate::render::pane_block(
        model,
        &format!("Connections ({})", screen.profiles.len()),
        focused,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenList, area);
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
    hits.register(HitTarget::ScreenDetail, area);
    if inner.height == 0 {
        return;
    }
    let visible = form.visible_rows(usize::from(inner.height).max(4), usize::from(inner.width));
    let lines: Vec<String> = visible.iter().map(|(_, line)| line.clone()).collect();
    // The sections' headings stand out from the fields under them.
    let heading = model
        .theme
        .style(crate::theme::Role::Muted, model.capabilities)
        .add_modifier(Modifier::BOLD);
    let drawn: Vec<Line> = lines
        .iter()
        .map(|line| {
            if line.starts_with(crate::screens::connection::HEADING) {
                Line::styled(line.clone(), heading)
            } else {
                Line::raw(line.clone())
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(drawn), inner);
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
            crate::render::show_input(frame, rect, &form.prefix(field), &value.value, value.secret);
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
        "Enter next  Left/Right pick a value  Space advanced  Esc cancel".into()
    } else {
        "Up/Down pick  n new  r Docker  Esc back".into()
    }
}
