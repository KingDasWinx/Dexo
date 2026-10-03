//! Connections: every saved connection beside what it points at, the databases found in
//! Docker, and the Add and Edit form in place of the details. It was a dialog of
//! seventy-two columns whose form scrolled through twenty-five fields.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crossterm::event::KeyCode;

use super::Button;
use super::widgets::{self, Chip, Entry, FieldRow};
use crate::model::Model;
use crate::mouse::{HitButton, HitMap, HitTarget};
use crate::screens::connections::{Item, State, driver_name, env_name};
use crate::theme::Role;

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
    let (list, detail) = super::list_and_detail(model, hits, area, items.len());
    list_pane(frame, list, model, hits, &items);
    if model.connection_form.open {
        form(frame, detail, model, hits);
        return;
    }
    let (title, rows) = picked_fields(model);
    let text = Text::from(widgets::field_lines(
        model,
        &rows,
        detail.width.saturating_sub(2),
    ));
    let footer: Vec<String> = screen.error.clone().into_iter().collect();
    super::detail_pane(
        frame,
        detail,
        model,
        hits,
        &title,
        &buttons(model),
        text,
        usize::from(super::detail_scroll(model)),
        &footer,
    );
}

/// The pane's title and fields for the pick: a connection with its state, a database
/// found in Docker, or a group.
fn picked_fields(model: &Model) -> (String, Vec<FieldRow>) {
    let screen = &model.connections;
    if let Some(group) = &screen.picked_group {
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
        return (
            group.clone(),
            vec![
                FieldRow::Field("Connections", names.len().to_string()),
                FieldRow::Field("Connected", connected.to_string()),
                FieldRow::Blank,
                FieldRow::Text(names.join(", ")),
            ],
        );
    }
    if let Some(database) = screen.picked_docker() {
        return (
            database.container.clone(),
            screen.detail_fields(model.active_session),
        );
    }
    let Some(profile) = screen.picked() else {
        return (String::new(), Vec::new());
    };
    let state = match screen.session_for(&profile.name) {
        Some(session) if model.active_session == Some(session.id) => " ◉ in use",
        Some(_) => " ● connected",
        None => "",
    };
    (
        format!("{}{state}", profile.name),
        screen.detail_fields(model.active_session),
    )
}

/// What acts on the whole screen rather than the pick.
pub fn toolbar_buttons() -> Vec<Button> {
    vec![
        Button::new(KeyCode::Char('u'), "From URL"),
        Button::new(KeyCode::Char('n'), "New"),
    ]
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
    // In use, Enter opens SQL on it, which is what New SQL does.
    let mut buttons = match (session.is_some(), in_use) {
        (false, _) => vec![Button::new(KeyCode::Enter, "Connect")],
        (true, false) => vec![Button::new(KeyCode::Enter, "Use")],
        (true, true) => vec![Button::new(KeyCode::Enter, "Open SQL")],
    };
    if !in_use {
        buttons.push(Button::new(KeyCode::Char('s'), "New SQL"));
    }
    buttons.extend([
        // The explorer shows what a session reads: offline there is nothing to browse.
        Button::new(KeyCode::Char('b'), "Browse").enabled_if(
            session.is_some(),
            format!("{} is not connected: Enter connects it.", profile.name),
        ),
        Button::new(KeyCode::Char('c'), "Disconnect").enabled_if(
            session.is_some(),
            format!("{} is not connected.", profile.name),
        ),
        Button::new(KeyCode::Char('e'), "Edit"),
        Button::new(KeyCode::Char('d'), "Duplicate"),
        Button::new(KeyCode::Char('t'), "Test"),
        Button::new(KeyCode::Char('y'), "Copy URL").enabled_if(
            crate::screens::connections::url_of(profile).is_some(),
            format!("A {} connection has no URL.", driver_name(&profile.driver)),
        ),
        Button::new(KeyCode::Char('x'), "Delete"),
    ]);
    buttons
}

/// The table's column widths in `width` cells. ADDRESS goes first when they do not fit,
/// then DRIVER; what a dropped column held is in the detail.
struct Columns {
    name: usize,
    driver: Option<usize>,
    env: usize,
    address: bool,
}

/// The least of an address worth showing.
const ADDRESS_MIN: usize = 12;

impl Columns {
    fn new(model: &Model, width: usize) -> Self {
        use unicode_width::UnicodeWidthStr;
        let screen = &model.connections;
        let mut names = Vec::new();
        let mut drivers = Vec::new();
        let mut envs = Vec::new();
        for cells in
            (0..screen.profiles.len()).filter_map(|index| screen.cells(index, model.active_session))
        {
            names.push(cells.name.width());
            drivers.push(cells.driver.width());
            envs.push(cells.env.width());
        }
        for database in screen.unsaved_docker() {
            names.push(database.container.width());
            drivers.push(driver_name(&database.connection.driver).width());
        }
        let widest = |widths: &[usize], least: usize, most: usize| {
            widths.iter().copied().max().unwrap_or(0).clamp(least, most)
        };
        let name = widest(&names, 4, 28);
        let driver = widest(&drivers, 6, 14);
        let env = widest(&envs, 3, 10);
        // The marker and the status glyph, then the name and the environment.
        let fixed = 2 + 2 + name + 2 + env;
        let driver_fits = fixed + 2 + driver <= width;
        Self {
            name,
            driver: driver_fits.then_some(driver),
            env,
            address: driver_fits && fixed + 2 + driver + 2 + ADDRESS_MIN <= width,
        }
    }

    /// A row's cells laid out under the header.
    fn spans(
        &self,
        glyph: Span<'static>,
        name: &str,
        driver: &str,
        env: Span<'static>,
        address: &str,
    ) -> Vec<Span<'static>> {
        let mut spans = vec![glyph, Span::raw(format!(" {}  ", cell(name, self.name)))];
        if let Some(width) = self.driver {
            spans.push(Span::raw(format!("{}  ", cell(driver, width))));
        }
        let env_text = cell(&env.content, self.env);
        spans.push(Span::styled(env_text, env.style));
        if self.address {
            spans.push(Span::raw(format!("  {address}")));
        }
        spans
    }

    fn header(&self) -> String {
        let mut header = format!("    {}  ", cell("NAME", self.name));
        if let Some(width) = self.driver {
            header.push_str(&format!("{}  ", cell("DRIVER", width)));
        }
        header.push_str(&cell("ENV", self.env));
        if self.address {
            header.push_str("  ADDRESS");
        }
        header
    }
}

/// `text` in exactly `width` cells: cut, or padded with spaces.
fn cell(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let text = crate::model::truncate_cell(text, width);
    let pad = width.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(pad))
}

/// The list as `items` has it, as a table under a pinned header: the connections in no
/// group, each group under its heading, then the databases found in Docker under theirs;
/// the pick kept in sight.
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
    let mut inner = block.inner(area);
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
    let width = usize::from(inner.width);
    let columns = Columns::new(model, width);
    let style = |role: Role| model.theme.style(role, model.capabilities);
    let heading = style(Role::Muted).add_modifier(Modifier::BOLD);
    if inner.height > 1 {
        frame.render_widget(
            Paragraph::new(crate::model::truncate_cell(&columns.header(), width)).style(heading),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        inner = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    }
    let cursor = screen.cursor(items);
    let mut entries: Vec<Entry> = Vec::new();
    let docker_heading = |entries: &mut Vec<Entry>| {
        entries.push(Entry::text(String::new(), false));
        entries.push(Entry::text("Found in Docker", true));
        let saved = screen.saved_docker();
        if !saved.is_empty() {
            entries.push(Entry::text(
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
                    spans: vec![Span::raw(format!(
                        "{} {name} ({count})",
                        if *folded { "▸" } else { "▾" }
                    ))],
                    target: Some(HitTarget::ListGroup(groups)),
                    picked,
                    heading: true,
                });
                groups += 1;
            }
            Item::Row(index) if *index < total => {
                let Some(cells) = screen.cells(*index, model.active_session) else {
                    continue;
                };
                let glyph = match cells.state {
                    State::InUse => Span::styled("◉", style(Role::Focus)),
                    State::Connected => Span::styled("●", style(Role::Success)),
                    State::Offline => Span::styled("○", style(Role::Muted)),
                };
                entries.push(Entry {
                    spans: columns.spans(
                        glyph,
                        &cells.name,
                        &cells.driver,
                        Span::styled(cells.env, style(cells.env_role)),
                        &cells.address,
                    ),
                    target: Some(HitTarget::ListRow(*index)),
                    picked,
                    heading: false,
                });
            }
            Item::Row(index) => {
                if !std::mem::replace(&mut docker, true) {
                    docker_heading(&mut entries);
                }
                if let Some(database) = screen.unsaved_docker().nth(index - total) {
                    let connection = &database.connection;
                    entries.push(Entry {
                        spans: columns.spans(
                            Span::styled("+", style(Role::Success)),
                            &database.container,
                            &driver_name(&connection.driver),
                            Span::raw(""),
                            &format!(
                                "{}:{}",
                                connection.host,
                                connection.port.unwrap_or_default()
                            ),
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
    widgets::entries(frame, inner, model, hits, &entries);
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
        "Up/Down pick  / search  o connected  v env  Left/Right fold  n new  u from URL  r Docker  Esc back"
            .into()
    }
}
