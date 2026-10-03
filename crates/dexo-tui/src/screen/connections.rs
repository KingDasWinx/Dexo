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
use super::widgets::{self, Chip};
use crate::model::Model;
use crate::mouse::{HitButton, HitMap, HitTarget};
use crate::screens::connections::{Item, env_name};

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let screen = &model.connections;
    if screen.profiles.is_empty() && screen.unsaved_docker().next().is_none() {
        if model.connection_form.open {
            form(frame, area, model, hits);
            return;
        }
        let mut lines = vec!["No connections yet.".to_string()];
        lines.extend(screen.error.clone());
        let mut buttons = toolbar_buttons();
        buttons.push(Button::new(KeyCode::Char('r'), "Look in Docker"));
        widgets::empty_with_buttons(frame, area, model, hits, &lines, &buttons);
        return;
    }
    // On a narrow screen the form takes it whole: under the list it scrolled twenty
    // fields through five rows.
    if model.connection_form.open && area.width < 100 {
        form(frame, area, model, hits);
        return;
    }
    let area = widgets::toolbar(
        frame,
        area,
        model,
        hits,
        Some(&screen.search),
        &chips(model),
        &toolbar_buttons(),
    );
    let items = screen.items();
    let (list, detail) = super::list_and_detail(area, items.len());
    list_pane(frame, list, model, hits, &items);
    if model.connection_form.open {
        form(frame, detail, model, hits);
        return;
    }
    let (title, lines) = if let Some(group) = &screen.picked_group {
        let names: Vec<&str> = screen
            .profiles
            .iter()
            .filter(|row| row.profile.group_path.as_deref().map(str::trim) == Some(group))
            .map(|row| row.profile.name.as_str())
            .collect();
        let connected = names
            .iter()
            .filter(|name| screen.session_for(name).is_some())
            .count();
        (
            group.clone(),
            vec![format!(
                "{} connections, {connected} connected: {}",
                names.len(),
                names.join(", ")
            )],
        )
    } else if let Some(database) = screen.picked_docker() {
        (
            database.container.clone(),
            screen.detail_lines(model.active_session),
        )
    } else if let Some(profile) = screen.picked() {
        (
            profile.name.clone(),
            screen.detail_lines(model.active_session),
        )
    } else {
        (String::new(), Vec::new())
    };
    let width = usize::from(detail.width.saturating_sub(2)).max(8);
    let lines: Vec<String> = lines
        .iter()
        .flat_map(|line| crate::model::wrap_words(line, width))
        .collect();
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

/// What acts on the whole screen rather than the pick.
fn toolbar_buttons() -> Vec<Button> {
    vec![Button::new(KeyCode::Char('n'), "New")]
}

/// The filters over the list.
fn chips(model: &Model) -> Vec<Chip> {
    let screen = &model.connections;
    vec![
        Chip {
            key: KeyCode::Char('o'),
            label: "Connected only".into(),
            active: screen.connected_only,
        },
        Chip {
            key: KeyCode::Char('v'),
            label: format!("Env: {}", screen.env.map_or("all", env_name)),
            active: screen.env.is_some(),
        },
    ]
}

/// What can be done to the pick: connect or use a connection, close its session, edit,
/// duplicate, test or delete it; add a database found in Docker as a connection; fold or
/// unfold a group.
pub fn buttons(model: &Model) -> Vec<Button> {
    let screen = &model.connections;
    if let Some(group) = &screen.picked_group {
        let folded = screen
            .items()
            .iter()
            .any(|item| matches!(item, Item::Group { name, folded: true, .. } if name == group));
        return vec![Button::new(
            KeyCode::Enter,
            if folded { "Unfold" } else { "Fold" },
        )];
    }
    if screen.picked_docker().is_some() {
        return vec![Button::new(KeyCode::Enter, "Add as connection")];
    }
    let Some(profile) = screen.picked() else {
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

/// One line of the list: its text, what a click on it picks, whether the pick is on it,
/// and whether it is a heading.
struct Entry {
    text: String,
    target: Option<HitTarget>,
    picked: bool,
    heading: bool,
}

/// The list as `items` has it: the connections in no group, each group under its
/// heading, then the databases found in Docker under theirs; the pick kept in sight.
fn list_pane(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap, items: &[Item]) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let screen = &model.connections;
    let focused = super::section(model) == super::Section::List;
    let total = screen.profiles.len();
    let title = if screen.filtered() {
        let shown = items
            .iter()
            .filter(|item| matches!(item, Item::Row(index) if *index < total))
            .count();
        format!("Connections ({shown} of {total})")
    } else {
        format!("Connections ({total})")
    };
    let block = crate::render::pane_block(model, &title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenList, area);
    if items.is_empty() {
        widgets::empty_with_buttons(
            frame,
            inner,
            model,
            hits,
            &["Nothing matches the filters.".to_string()],
            &[Button::new(KeyCode::Esc, "Clear filters")],
        );
        return;
    }
    let cursor = screen.cursor(items);
    let mut entries: Vec<Entry> = Vec::new();
    let text = |text: String, heading: bool| Entry {
        text,
        target: None,
        picked: false,
        heading,
    };
    let docker_heading = |entries: &mut Vec<Entry>| {
        entries.push(text(String::new(), false));
        entries.push(text("Found in Docker".into(), true));
        let saved = screen.saved_docker();
        if !saved.is_empty() {
            entries.push(text(
                format!("already saved as connections: {}", saved.join(", ")),
                false,
            ));
        }
    };
    let mut groups = 0;
    let mut docker = false;
    for (at, item) in items.iter().enumerate() {
        let picked = cursor == Some(at);
        match item {
            Item::Group {
                name,
                count,
                folded,
            } => {
                entries.push(Entry {
                    text: format!("{} {name} ({count})", if *folded { "▸" } else { "▾" }),
                    target: Some(HitTarget::ListGroup(groups)),
                    picked,
                    heading: true,
                });
                groups += 1;
            }
            Item::Row(index) if *index < total => entries.push(Entry {
                text: screen.row_text(*index, model.active_session),
                target: Some(HitTarget::ListRow(*index)),
                picked,
                heading: false,
            }),
            Item::Row(index) => {
                if !std::mem::replace(&mut docker, true) {
                    docker_heading(&mut entries);
                }
                if let Some(database) = screen.unsaved_docker().nth(index - total) {
                    let connection = &database.connection;
                    entries.push(Entry {
                        text: format!(
                            "+ {} [{}] {}:{}",
                            database.container,
                            connection.driver,
                            connection.host,
                            connection.port.unwrap_or_default()
                        ),
                        target: Some(HitTarget::ListRow(*index)),
                        picked,
                        heading: false,
                    });
                }
            }
        }
    }
    // Containers whose connection is saved are still named, filters off.
    if !docker && !screen.filtered() && !screen.saved_docker().is_empty() {
        docker_heading(&mut entries);
    }
    let visible = usize::from(inner.height);
    let picked = entries.iter().position(|entry| entry.picked).unwrap_or(0);
    let offset = crate::palette::scroll_to_selection(picked, 0, entries.len(), visible);
    let width = usize::from(inner.width);
    let heading = model
        .theme
        .style(crate::theme::Role::Muted, model.capabilities)
        .add_modifier(Modifier::BOLD);
    let lines: Vec<Line> = entries
        .iter()
        .skip(offset)
        .take(visible)
        .map(|entry| {
            // The pick is marked as well as reversed, so it reads without colour.
            let marker = if entry.picked { "> " } else { "  " };
            let text = crate::model::truncate_cell(&format!("{marker}{}", entry.text), width);
            if entry.picked {
                Line::styled(
                    format!("{text:<width$}"),
                    Style::default().add_modifier(Modifier::REVERSED),
                )
            } else if entry.heading {
                Line::styled(text, heading)
            } else {
                Line::raw(text)
            }
        })
        .collect();
    for (line, entry) in entries.iter().skip(offset).take(visible).enumerate() {
        if let Some(target) = entry.target {
            hits.register(target, crate::mouse::line_rect(inner, line));
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
    } else if model.connections.search.typing {
        "Type to search  Up/Down pick  Enter keep  Esc clear".into()
    } else {
        "Up/Down pick  / search  o connected  v env  Left/Right fold  n new  r Docker  Esc back"
            .into()
    }
}
