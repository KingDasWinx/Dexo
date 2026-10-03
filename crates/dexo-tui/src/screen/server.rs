//! Server: the sessions of the server a connection reaches, read again every two seconds
//! while on screen. It was Inspect Sessions, a dialog that cut every query at forty
//! characters on a terminal of a hundred and sixty.

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};

use super::Button;
use super::widgets::{self, Chip, Entry, FieldRow};
use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};
use crate::screens::admin::{ServerView, SessionSort, duration, one_line};
use crate::theme::Role;

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let admin = &model.admin;
    let Some(server) = &admin.server else {
        let back = crate::palette::shortcut_for(model, "screen.workbench", None)
            .map(|key| format!(" ({key})"))
            .unwrap_or_default();
        super::empty_state(
            frame,
            area,
            model,
            &[
                "No connection is open.".to_string(),
                format!(
                    "Connect to a server on the Workbench{back}: its sessions are listed here."
                ),
            ],
        );
        return;
    };
    let views: Vec<(String, bool)> = ServerView::ALL
        .into_iter()
        .map(|view| (view.title().to_string(), admin.view == view))
        .collect();
    let rest = super::views_and_toolbar(
        frame,
        area,
        model,
        hits,
        &views,
        None,
        &[Chip {
            key: KeyCode::Char('c'),
            label: format!("Server: {}", server.connection),
            active: false,
        }],
        &toolbar_buttons(model),
    );
    let on_sessions = admin.view == ServerView::Sessions;
    let chips = if on_sessions {
        vec![Chip {
            key: KeyCode::Char('a'),
            label: if admin.show_idle {
                "Idle: shown".into()
            } else {
                "Idle: hidden".into()
            },
            active: admin.show_idle,
        }]
    } else {
        Vec::new()
    };
    let rest = widgets::toolbar(frame, rest, model, hits, Some(&admin.search), &chips, &[]);
    if on_sessions {
        sessions(frame, rest, model, hits);
    } else {
        view_table(frame, rest, model, hits);
    }
}

/// A view other than Sessions: its rows in a table, and the picked one whole under it.
fn view_table(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let admin = &model.admin;
    let view = admin.view;
    let Some(_) = &admin.rows else {
        let mut lines = vec![match &admin.last_error {
            Some(_) => format!(
                "The server's {} could not be read.",
                view.title().to_lowercase()
            ),
            None => format!("Reading the server's {}...", view.title().to_lowercase()),
        }];
        lines.extend(admin.last_error.clone());
        super::empty_state(frame, area, model, &lines);
        return;
    };
    let cells = admin.view_cells();
    if cells.is_empty() {
        if admin.search.input.is_empty() {
            let mut lines = vec![format!("No {} to show.", view.title().to_lowercase())];
            lines.extend(admin.restriction.clone());
            super::empty_state(frame, area, model, &lines);
        } else {
            widgets::empty_with_buttons(
                frame,
                area,
                model,
                hits,
                &["Nothing matches the search.".to_string()],
                &[Button::new(KeyCode::Esc, "Clear filters")],
            );
        }
        return;
    }
    let columns = view.columns();
    let detail_rows = (columns.len() as u16 + 2).min(area.height / 3);
    let table = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(detail_rows),
    );
    if table.width >= 2 && table.height >= 2 {
        let block = crate::render::pane_block(
            model,
            &format!("{} ({}) · {}", view.title(), cells.len(), admin.freshness()),
            super::section(model) == super::Section::List,
        );
        let mut inner = block.inner(table);
        frame.render_widget(block, table);
        hits.register(HitTarget::ScreenList, table);
        // Each column as wide as its widest cell, up to forty; the last takes the rest.
        let widths: Vec<usize> = (0..columns.len())
            .map(|column| {
                cells
                    .iter()
                    .map(|row| unicode_width::UnicodeWidthStr::width(row[column].as_str()))
                    .chain(std::iter::once(columns[column].len()))
                    .max()
                    .unwrap_or(0)
                    .min(40)
            })
            .collect();
        let line = |row: &[String]| -> String {
            row.iter()
                .enumerate()
                .map(|(column, text)| {
                    if column + 1 == row.len() {
                        text.clone()
                    } else {
                        let text = crate::model::truncate_cell(text, widths[column]);
                        let pad = widths[column]
                            .saturating_sub(unicode_width::UnicodeWidthStr::width(text.as_str()));
                        format!("{text}{}  ", " ".repeat(pad))
                    }
                })
                .collect()
        };
        if inner.height > 1 {
            let header: Vec<String> = columns.iter().map(|name| name.to_string()).collect();
            frame.render_widget(
                ratatui::widgets::Paragraph::new(crate::model::truncate_cell(
                    &format!("  {}", line(&header)),
                    usize::from(inner.width),
                ))
                .style(
                    model
                        .theme
                        .style(Role::Muted, model.capabilities)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Rect::new(inner.x, inner.y, inner.width, 1),
            );
            inner = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
        }
        let entries: Vec<Entry> = cells
            .iter()
            .enumerate()
            .map(|(index, row)| Entry {
                spans: vec![Span::raw(line(row))],
                target: Some(HitTarget::ListRow(index)),
                picked: index == admin.view_selected,
                heading: false,
            })
            .collect();
        widgets::entries(frame, inner, model, hits, &entries);
    }
    if detail_rows >= 3
        && let Some(row) = cells.get(admin.view_selected)
    {
        let detail = Rect::new(area.x, table.bottom(), area.width, detail_rows);
        let fields: Vec<FieldRow> = columns
            .iter()
            .zip(row)
            .map(|(column, value)| FieldRow::Field(label(column), value.clone()))
            .collect();
        let footer: Vec<String> = admin.restriction.clone().into_iter().collect();
        super::detail_pane(
            frame,
            detail,
            model,
            hits,
            view.title(),
            &[],
            Text::from(widgets::field_lines(
                model,
                &fields,
                detail.width.saturating_sub(2),
            )),
            usize::from(super::detail_scroll(model)),
            &footer,
        );
    }
}

/// A column's name as a field's label.
fn label(column: &str) -> &'static str {
    match column {
        "PID" => "Session",
        "TYPE" => "Type",
        "RELATION" => "Relation",
        "MODE" => "Mode",
        "STATE" => "State",
        "OBJECT" => "Object",
        "SIZE" => "Size",
        "NAME" => "Name",
        "VALUE" => "Value",
        "SCOPE" => "Scope",
        _ => "",
    }
}

/// Pause or resume the reading, and read now.
pub fn toolbar_buttons(model: &Model) -> Vec<Button> {
    if model.admin.server.is_none() {
        return Vec::new();
    }
    vec![
        Button::new(
            KeyCode::Char('p'),
            if model.admin.paused {
                "Resume"
            } else {
                "Pause"
            },
        ),
        Button::new(KeyCode::Char('r'), "Refresh"),
    ]
}

/// What can be done to the picked session: stop its query, end it, copy or open its
/// query. Dexo's own session is not stopped from here, nor anything on a read-only
/// connection.
pub fn buttons(model: &Model) -> Vec<Button> {
    let admin = &model.admin;
    // A question asked of the session has the pane until it is answered; the other
    // views have no session picked.
    if admin.terminate.is_some() || admin.cancel.is_some() || admin.view != ServerView::Sessions {
        return Vec::new();
    }
    let Some(session) = admin.picked() else {
        return Vec::new();
    };
    let connection = admin
        .server
        .as_ref()
        .map(|server| server.connection.as_str())
        .unwrap_or_default();
    let refusal = if admin.is_you(session) {
        Some("This is Dexo's own session: stopping it stops your own work.".to_string())
    } else if admin.read_only {
        Some(format!("{connection} is read-only: it stops no session."))
    } else {
        None
    };
    let guarded = |button: Button| match &refusal {
        Some(why) => button.disabled(why.clone()),
        None => button,
    };
    let query = session.current_query.is_some();
    let no_query = format!("Session {} runs no query.", session.id);
    vec![
        guarded(Button::new(KeyCode::Char('k'), "Cancel query")),
        guarded(Button::new(KeyCode::Char('t'), "Terminate…")),
        Button::new(KeyCode::Char('y'), "Copy query").enabled_if(query, no_query.clone()),
        Button::new(KeyCode::Char('o'), "Open in editor").enabled_if(query, no_query),
    ]
}

fn sessions(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let admin = &model.admin;
    if admin.sessions.is_empty() {
        let mut lines = vec![if admin.loading {
            "Reading the server's sessions...".to_string()
        } else {
            "No sessions.".to_string()
        }];
        lines.extend(admin.last_error.clone());
        super::empty_state(frame, area, model, &lines);
        return;
    }
    let shown = admin.visible();
    if shown.is_empty() {
        let (line, button) = if admin.search.input.is_empty() && !admin.show_idle {
            (
                "No session is running a query.",
                Button::new(KeyCode::Char('a'), "Show idle"),
            )
        } else {
            (
                "Nothing matches the filters.",
                Button::new(KeyCode::Esc, "Clear filters"),
            )
        };
        widgets::empty_with_buttons(frame, area, model, hits, &[line.to_string()], &[button]);
        return;
    }
    let list_rows = (shown.len() as u16 + 3).clamp(5, (area.height * 3 / 5).max(5));
    let [list, detail] =
        Layout::vertical([Constraint::Length(list_rows), Constraint::Min(0)]).areas(area);
    list_pane(frame, list, model, hits, &shown);
    detail_pane(frame, detail, model, hits);
}

/// The column names, the sorted one marked; `narrow` drops who and where, which the
/// detail still says.
fn header(sort: SessionSort, narrow: bool) -> String {
    let mark = |name: &str, column: SessionSort| {
        if sort == column {
            format!("{name} ▼")
        } else {
            name.to_string()
        }
    };
    let mut header = format!("    {:<8} ", mark("PID", SessionSort::Pid));
    if !narrow {
        header.push_str(&format!(
            "{:<12} {:<12} ",
            mark("USER", SessionSort::User),
            mark("DATABASE", SessionSort::Database)
        ));
    }
    header.push_str(&format!(
        "{:<26} {:>8}  QUERY",
        mark("STATE", SessionSort::State),
        mark("TIME", SessionSort::Time)
    ));
    header
}

fn list_pane(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    shown: &[&dexo_driver_api::SessionInfo],
) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let admin = &model.admin;
    let count = if admin.filtered() || shown.len() != admin.sessions.len() {
        format!("{} of {}", shown.len(), admin.sessions.len())
    } else {
        shown.len().to_string()
    };
    let blocked = if admin.blocking.is_empty() {
        String::new()
    } else {
        format!(" · {} blocked", admin.blocking.len())
    };
    let block = crate::render::pane_block(
        model,
        &format!("Sessions ({count}){blocked} · {}", admin.freshness()),
        super::section(model) == super::Section::List,
    );
    let mut inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenList, area);
    let narrow = inner.width < 100;
    let style = |role: Role| model.theme.style(role, model.capabilities);
    if inner.height > 1 {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(crate::model::truncate_cell(
                &header(admin.sort, narrow),
                usize::from(inner.width),
            ))
            .style(style(Role::Muted).add_modifier(ratatui::style::Modifier::BOLD)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        inner = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    }
    let cell = |text: &str, width: usize| {
        let text = crate::model::truncate_cell(text, width);
        let pad = width.saturating_sub(unicode_width::UnicodeWidthStr::width(text.as_str()));
        format!("{text}{}", " ".repeat(pad))
    };
    let entries: Vec<Entry> = shown
        .iter()
        .enumerate()
        .map(|(index, session)| {
            let note = admin.blocking_note(&session.id);
            let glyph = if note
                .as_deref()
                .is_some_and(|note| note.starts_with("blocked"))
            {
                Span::styled("⊘", style(Role::Warning))
            } else if session.state == "active" {
                Span::styled("●", style(Role::Success))
            } else {
                Span::styled("●", style(Role::Muted))
            };
            let state = if admin.is_you(session) {
                format!("{} · you", session.state)
            } else {
                session.state.clone()
            };
            let query = one_line(session.current_query.as_deref().unwrap_or("-"));
            let query = match note {
                Some(note) => format!("[{note}] {query}"),
                None => query,
            };
            let mut text = format!(" {} ", cell(&session.id, 8));
            if !narrow {
                text.push_str(&format!(
                    "{} {} ",
                    cell(session.user.as_deref().unwrap_or("-"), 12),
                    cell(session.database.as_deref().unwrap_or("-"), 12)
                ));
            }
            text.push_str(&format!(
                "{} {:>8}  {query}",
                cell(&state, 26),
                session
                    .duration_ms
                    .map(duration)
                    .unwrap_or_else(|| "-".into())
            ));
            Entry {
                spans: vec![glyph, Span::raw(text)],
                target: Some(HitTarget::ListRow(index)),
                picked: index == admin.selected,
                heading: false,
            }
        })
        .collect();
    widgets::entries(frame, inner, model, hits, &entries);
}

/// The picked session: its query in colour, who and where it is, and what it blocks or
/// waits for; under them a question being asked of it.
fn detail_pane(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let admin = &model.admin;
    let Some(session) = admin.picked() else {
        return;
    };
    let width = area.width.saturating_sub(2);
    let driver = admin
        .server
        .as_ref()
        .and_then(|server| {
            model
                .connections
                .profiles
                .iter()
                .find(|row| row.profile.name == server.connection)
        })
        .map_or("postgres", |row| row.profile.driver.as_str());
    let mut text: Vec<Line> = crate::widgets::editor::sql_lines(
        session.current_query.as_deref().unwrap_or("-"),
        usize::from(width.saturating_sub(1)),
        dexo_app::dialect_for_driver(driver),
    )
    .into_iter()
    .map(|line| {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(line.spans);
        Line::from(spans)
    })
    .collect();
    let mut fields = vec![
        FieldRow::Section("Session"),
        FieldRow::Field("User", session.user.clone().unwrap_or_else(|| "-".into())),
        FieldRow::Field(
            "Database",
            session.database.clone().unwrap_or_else(|| "-".into()),
        ),
        FieldRow::Field(
            "State",
            if admin.is_you(session) {
                format!("{} · Dexo's own", session.state)
            } else {
                session.state.clone()
            },
        ),
        FieldRow::Field(
            "Running",
            session
                .duration_ms
                .map(duration)
                .unwrap_or_else(|| "-".into()),
        ),
    ];
    if let Some(client) = &session.client {
        fields.push(FieldRow::Field("Client", client.clone()));
    }
    if let Some(application) = &session.application {
        fields.push(FieldRow::Field("Program", application.clone()));
    }
    let blocking = admin.blocking_of(&session.id);
    if !blocking.is_empty() {
        fields.push(FieldRow::Section("Blocking"));
        fields.extend(blocking.into_iter().map(FieldRow::Text));
    }
    text.push(Line::default());
    text.extend(widgets::field_lines(model, &fields, width));
    let mut footer = Vec::new();
    if let Some(prompt) = &admin.terminate {
        footer.extend(prompt.lines());
    } else if let Some(prompt) = &admin.cancel {
        footer.extend(prompt.lines());
    } else {
        footer.extend(admin.last_error.clone());
        footer.extend(admin.notice.clone());
    }
    let drawn = super::detail_pane(
        frame,
        area,
        model,
        hits,
        &format!("Session {}", session.id),
        &buttons(model),
        Text::from(text),
        usize::from(admin.detail_scroll),
        &footer,
    );
    hits.set_scroll_limit(crate::mouse::ScrollArea::Sessions, drawn.max_scroll);
    hits.set_page(crate::mouse::ScrollArea::Sessions, drawn.page);
    for (index, line) in footer
        .iter()
        .enumerate()
        .take(usize::from(drawn.footer.height))
    {
        let rect = crate::mouse::line_rect(drawn.footer, index);
        if let Some(prompt) = &admin.terminate {
            if line.starts_with("id:") {
                hits.register(HitTarget::FormField(0), rect);
                if prompt.footer == crate::widgets::form::FooterFocus::Input {
                    crate::render::show_input(frame, rect, "id: ", &prompt.typed, false);
                }
            } else if line.contains("[Cancel]") {
                crate::widgets::form::register_footer(hits, rect, line, "Terminate");
            }
        } else if admin.cancel.is_some() && line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Confirm");
        }
    }
}

pub fn hints(model: &Model) -> String {
    let admin = &model.admin;
    if admin.terminate.is_some() {
        "type the id  Enter terminate  Esc cancel".into()
    } else if admin.cancel.is_some() {
        "Left/Right pick  Enter answer  Esc keep it running".into()
    } else if admin.search.typing {
        "Type to search  Up/Down pick  Enter keep  Esc clear".into()
    } else if admin.view == ServerView::Sessions {
        "Up/Down pick  / search  a idle  s sort  c server  1-5 views  Esc back".into()
    } else {
        "Up/Down pick  / search  c server  1-5 views  Esc back".into()
    }
}
