//! Agents: the writes agents wait to make, what they have done, and the profiles they
//! connect through -- what each reads and sees, what it may write on each connection,
//! and which agents start Dexo with it.

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::Button;
use super::widgets::{self, Entry, FieldRow};
use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};
use crate::screens::mcp_profiles::{
    AgentForm, Form, ProfileForm, ProfileRow, Row, Sees, TableState, WritesForm, WritesRow,
};
use crate::theme::Role;
use crate::widgets::panel::{self, Lines};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AgentsView {
    #[default]
    Approvals,
    Activity,
    Profiles,
}

impl AgentsView {
    pub const ALL: [AgentsView; 3] = [
        AgentsView::Approvals,
        AgentsView::Activity,
        AgentsView::Profiles,
    ];

    pub fn title(self) -> &'static str {
        match self {
            AgentsView::Approvals => "Approvals",
            AgentsView::Activity => "Activity",
            AgentsView::Profiles => "Profiles",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|view| *view == self).unwrap_or(0)
    }

    /// The view `step` places away, wrapping.
    pub fn step(self, step: isize) -> Self {
        let count = Self::ALL.len() as isize;
        Self::ALL[(self.index() as isize + step).rem_euclid(count) as usize]
    }
}

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let waiting = model.mcp_audit.pending.len();
    let views: Vec<(String, bool)> = AgentsView::ALL
        .into_iter()
        .map(|view| {
            let label = match view {
                AgentsView::Approvals if waiting > 0 => format!("{} {waiting}", view.title()),
                _ => view.title().to_string(),
            };
            (label, model.agents_view == view)
        })
        .collect();
    let on_activity = model.agents_view == AgentsView::Activity;
    let body = super::views_and_toolbar(
        frame,
        area,
        model,
        hits,
        &views,
        on_activity.then_some(&model.mcp_audit.search),
        &if on_activity {
            activity_chips(model)
        } else {
            Vec::new()
        },
        &[],
    );
    match model.agents_view {
        AgentsView::Approvals => approvals(frame, body, model, hits),
        AgentsView::Activity => activity(frame, body, model, hits),
        AgentsView::Profiles => profiles(frame, body, model, hits),
    }
}

/// What can be done to the picked request: approve it, or deny it.
pub fn approval_buttons(model: &Model) -> Vec<Button> {
    let audit = &model.mcp_audit;
    if audit.current().is_none() || audit.deciding.is_some() {
        return Vec::new();
    }
    let mut buttons = vec![
        Button::new(KeyCode::Char('a'), "Approve"),
        Button::new(KeyCode::Char('d'), "Deny"),
    ];
    if audit
        .current()
        .is_some_and(|request| !request.operations.is_empty())
    {
        buttons.push(Button::new(KeyCode::Char('A'), "Approve, stop asking"));
    }
    buttons
}

/// Who may ask for a write, a line each: what the empty queue has to say.
fn who_may_ask(model: &Model) -> Vec<String> {
    let lines: Vec<String> = model
        .mcp_profiles
        .profiles
        .iter()
        .filter(|profile| profile.enabled)
        .flat_map(|profile| {
            profile
                .connections
                .iter()
                .filter(|connection| connection.refused.is_none())
                .filter_map(move |connection| {
                    connection.access.as_ref().map(|access| {
                        format!(
                            "{} may write on {}, {}.",
                            profile.name,
                            connection.name,
                            access.left(model.mcp_audit.now)
                        )
                    })
                })
        })
        .collect();
    if lines.is_empty() {
        vec!["No agent may write: w on a profile's connection lets one.".into()]
    } else {
        lines
    }
}

fn approvals(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let audit = &model.mcp_audit;
    if audit.pending.is_empty() {
        let mut lines = vec!["No write is waiting.".to_string()];
        lines.extend(who_may_ask(model));
        lines.extend(audit.notice.clone());
        super::empty_state(frame, area, model, &lines);
        return;
    }
    let rows: Vec<String> = audit
        .pending
        .iter()
        .map(|request| audit.row(request))
        .collect();
    let (list, detail) = super::list_and_detail(model, hits, area, rows.len());
    super::list_pane(
        frame,
        list,
        model,
        hits,
        &format!("Waiting for you ({})", rows.len()),
        None,
        &rows,
        audit.position(),
    );
    let width = detail.width.saturating_sub(2);
    let mut text = Vec::new();
    if let Some(request) = audit.current() {
        let gone = if request.seems_gone(audit.now) {
            " (the agent has stopped answering)"
        } else {
            ""
        };
        let environment = connection_environment(model, &request.connection);
        text.extend(widgets::field_lines(
            model,
            &[
                FieldRow::Field("Profile", request.profile.clone()),
                FieldRow::Field("Wants", crate::screens::mcp_audit::wants(request)),
                FieldRow::Field(
                    "Connection",
                    match environment {
                        Some(environment) => format!("{} · {environment}", request.connection),
                        None => request.connection.clone(),
                    },
                ),
                FieldRow::Field("Tables", request.targets.join(", ")),
                FieldRow::Field(
                    "Asked",
                    crate::screens::mcp_audit::clock(request.created_at),
                ),
                FieldRow::Field(
                    "Left",
                    format!(
                        "{}{gone}",
                        crate::screens::mcp_profiles::duration_words(
                            request.seconds_left(audit.now)
                        )
                    ),
                ),
            ],
            width,
        ));
        text.push(Line::default());
        // What it would run, whole and in the editor's colours: a statement cut at a
        // popup's edge was approved unseen.
        let statement = crate::screens::mcp_audit::readable_statement(request);
        let sql = request.statement == statement;
        let lines: Vec<Line> = if sql {
            crate::widgets::editor::sql_lines(
                &statement,
                usize::from(width.saturating_sub(1)),
                dialect_of(model, &request.connection),
            )
        } else {
            statement
                .lines()
                .flat_map(|line| crate::model::wrap_display_text(line, usize::from(width)))
                .map(Line::raw)
                .collect()
        };
        text.extend(lines.into_iter().map(|line| {
            let mut spans = vec![Span::raw(" ")];
            spans.extend(line.spans);
            Line::from(spans)
        }));
    }
    let footer = audit.decision_lines(usize::from(width));
    let drawn = super::detail_pane(
        frame,
        detail,
        model,
        hits,
        "Request",
        &approval_buttons(model),
        ratatui::text::Text::from(text),
        usize::from(audit.scroll),
        &footer,
    );
    hits.set_scroll_limit(crate::mouse::ScrollArea::McpAudit, drawn.max_scroll);
    hits.set_page(crate::mouse::ScrollArea::McpAudit, drawn.page);
    if let Some(deciding) = &audit.deciding {
        let label = if deciding.approve { "Approve" } else { "Deny" };
        register_buttons(hits, drawn.footer, &footer, label);
    }
}

/// The saved connection called `name`.
fn saved_connection<'a>(model: &'a Model, name: &str) -> Option<&'a dexo_app::ConnectionProfile> {
    model
        .connections
        .profiles
        .iter()
        .find(|row| row.profile.name == name)
        .map(|row| &row.profile)
}

/// The environment of the saved connection called `name`, unless it is plain local.
fn connection_environment(model: &Model, name: &str) -> Option<String> {
    let environment = &saved_connection(model, name)?.environment;
    crate::accessibility::environment_role(environment).map(|_| environment.clone())
}

/// The SQL dialect of the saved connection called `name`, for a statement's colours.
fn dialect_of(model: &Model, name: &str) -> dexo_sql::Dialect {
    let driver =
        saved_connection(model, name).map_or("postgres", |profile| profile.driver.as_str());
    dexo_app::dialect_for_driver(driver)
}

/// The filters over the calls: one profile's, and how they went.
fn activity_chips(model: &Model) -> Vec<widgets::Chip> {
    let audit = &model.mcp_audit;
    vec![
        widgets::Chip {
            key: KeyCode::Char('p'),
            label: format!("Profile: {}", audit.profile.as_deref().unwrap_or("all")),
            active: audit.profile.is_some(),
        },
        widgets::Chip {
            key: KeyCode::Char('o'),
            label: format!(
                "Outcome: {}",
                audit.outcome.map_or("all", |outcome| outcome.word())
            ),
            active: audit.outcome.is_some(),
        },
    ]
}

fn activity(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    use crate::screens::mcp_audit::CallOutcome;
    let audit = &model.mcp_audit;
    let events = audit.visible_events();
    if events.is_empty() {
        if audit.events.is_empty() {
            super::empty_state(
                frame,
                area,
                model,
                &[
                    "No calls yet.".to_string(),
                    "What agents do through Dexo is listed here as it happens.".to_string(),
                ],
            );
        } else {
            widgets::empty_with_buttons(
                frame,
                area,
                model,
                hits,
                &["Nothing matches the filters.".to_string()],
                &[Button::new(KeyCode::Esc, "Clear filters")],
            );
        }
        return;
    }
    let picked = audit.event_selected.min(events.len() - 1);
    // The table above, the picked call in full below it.
    let (table, detail) = super::table_and_detail(model, hits, area, 5.min(area.height / 3));
    if table.width >= 2 && table.height >= 2 {
        let title = if audit.filtered() {
            format!("Recent calls ({} shown)", events.len())
        } else {
            format!("Recent calls ({})", events.len())
        };
        let block =
            crate::render::pane_block(model, &title, super::section(model) == super::Section::List);
        let mut inner = block.inner(table);
        frame.render_widget(block, table);
        hits.register(HitTarget::ScreenList, table);
        let width = usize::from(inner.width);
        let target_width = width
            .saturating_sub(4 + 8 + 2 + 12 + 2 + 20 + 2 + 2 + 24 + 2 + 7 + 2 + 6)
            .max(8);
        let header = format!(
            "    {:<8}  {:<12}  {:<20}  {:<target_width$}  {:<24}  {:>7}  {:>6}",
            "TIME", "PROFILE", "TOOL", "TARGET", "OUTCOME", "MS", "ROWS"
        );
        if inner.height > 1 {
            frame.render_widget(
                Paragraph::new(crate::model::truncate_cell(&header, width)).style(
                    model
                        .theme
                        .style(Role::Muted, model.capabilities)
                        .add_modifier(Modifier::BOLD),
                ),
                Rect::new(inner.x, inner.y, inner.width, 1),
            );
            inner = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
        }
        let style = |role: Role| model.theme.style(role, model.capabilities);
        let entries: Vec<Entry> = events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                let kind = event.kind();
                let role = match kind {
                    CallOutcome::Ok => Role::Success,
                    CallOutcome::Failed => Role::Error,
                    CallOutcome::Denied => Role::Warning,
                    CallOutcome::Waiting => Role::Muted,
                };
                Entry {
                    spans: vec![
                        Span::styled(kind.glyph(), style(role)),
                        Span::raw(format!(
                            " {:<8}  {:<12}  {:<20}  {:<target_width$}  {:<24}  {:>7}  {:>6}",
                            event.time,
                            crate::model::truncate_cell(&event.profile, 12),
                            crate::model::truncate_cell(&event.tool, 20),
                            crate::model::truncate_cell(&event.target, target_width),
                            crate::model::truncate_cell(&event.outcome, 24),
                            event.duration_ms,
                            event.rows
                        )),
                    ],
                    target: Some(HitTarget::ListRow(index)),
                    picked: index == picked,
                    heading: false,
                }
            })
            .collect();
        widgets::entries(frame, inner, model, hits, &entries);
    }
    if detail.height >= 3 {
        let event = events[picked];
        let on = if event.target.is_empty() {
            String::new()
        } else {
            format!(" on {}", event.target)
        };
        let fields = [
            FieldRow::Field("Call", format!("{}{on}", event.tool)),
            FieldRow::Field(
                "By",
                if event.client.is_empty() {
                    format!("{} at {}", event.profile, event.time)
                } else {
                    format!(
                        "{} through {} at {}",
                        event.profile, event.client, event.time
                    )
                },
            ),
            FieldRow::Field(
                "Result",
                format!(
                    "{} · {} ms · {} row{}",
                    event.outcome,
                    event.duration_ms,
                    event.rows,
                    if event.rows == 1 { "" } else { "s" }
                ),
            ),
        ];
        super::detail_pane(
            frame,
            detail,
            model,
            hits,
            "Call",
            &[],
            ratatui::text::Text::from(widgets::field_lines(
                model,
                &fields,
                detail.width.saturating_sub(2),
            )),
            usize::from(super::detail_scroll(model)),
            &[],
        );
    }
}

/// What acts on the profiles as a whole, over their list.
pub fn list_buttons(model: &Model) -> Vec<Button> {
    match model.agents_view {
        AgentsView::Profiles => vec![
            Button::new(KeyCode::Char('n'), "New"),
            Button::new(KeyCode::Char('R'), "Stop all writes"),
        ],
        _ => Vec::new(),
    }
}

/// What can be done to the picked profile; its connections and agents say their own keys.
pub fn profile_buttons(model: &Model) -> Vec<Button> {
    let screen = &model.mcp_profiles;
    // A form or a question has the pane until it is done.
    let Some(profile) = screen.selected_profile() else {
        return Vec::new();
    };
    if screen.form.is_some() || screen.confirm.is_some() {
        return Vec::new();
    }
    vec![
        Button::new(
            KeyCode::Char('e'),
            if profile.enabled { "Disable" } else { "Enable" },
        ),
        Button::new(KeyCode::Enter, "Edit"),
        Button::new(KeyCode::Char('x'), "Delete"),
    ]
}

fn profiles(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let screen = &model.mcp_profiles;
    if screen.profiles.is_empty() {
        // No list to sit beside: the form has the view.
        if let Some(form) = &screen.form {
            draw_form(frame, area, model, form, hits);
            return;
        }
        let mut lines: Vec<String> = crate::screens::mcp_profiles::McpProfilesScreen::EMPTY
            .iter()
            .map(|line| line.to_string())
            .collect();
        if !screen.status.is_empty() {
            lines.push(String::new());
            lines.extend(screen.status.lines().map(str::to_string));
        }
        widgets::empty_with_buttons(
            frame,
            area,
            model,
            hits,
            &lines,
            &[Button::new(KeyCode::Char('n'), "New profile")],
        );
        return;
    }
    let (list, detail) = super::list_and_detail(model, hits, area, screen.profiles.len());
    profiles_list(frame, list, model, hits);
    // A form takes the pane the profile was in, until it is done.
    match &screen.form {
        Some(form) => draw_form(frame, detail, model, form, hits),
        None => sheet(frame, detail, model, hits),
    }
}

/// Each profile: whether it is enabled, the agents that use it, whether it may write.
fn profiles_list(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let screen = &model.mcp_profiles;
    let block = crate::render::pane_block(
        model,
        &format!("Profiles ({})", screen.profiles.len()),
        super::section(model) == super::Section::List,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenList, area);
    let inner = widgets::list_actions(frame, inner, model, hits, &list_buttons(model));
    let style = |role: Role| model.theme.style(role, model.capabilities);
    let name_width = screen
        .profiles
        .iter()
        .map(|profile| profile.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(24);
    let compact = inner.width < 40;
    let entries: Vec<Entry> = screen
        .profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let agents = match model.mcp_clients.of(&profile.name).as_slice() {
                [] => "no agent".to_string(),
                [one] => one.client.name().to_string(),
                many => format!("{} agents", many.len()),
            };
            // A disabled profile serves no agent, whatever access it keeps.
            let state = if !profile.enabled {
                Span::styled("disabled", style(Role::Muted))
            } else if profile.writes() > 0 && profile.unasked() {
                // Something of it runs without asking: said where the list is read.
                Span::styled("✎ writes, auto", style(Role::Warning))
            } else if profile.writes() > 0 {
                Span::raw("✎ writes")
            } else {
                Span::styled("read only", style(Role::Muted))
            };
            let mut spans = vec![
                if profile.enabled {
                    Span::styled("●", style(Role::Success))
                } else {
                    Span::styled("○", style(Role::Muted))
                },
                Span::raw(format!(
                    " {:<name_width$}  ",
                    crate::model::truncate_cell(&profile.name, name_width)
                )),
            ];
            if compact {
                if profile.enabled && profile.writes() > 0 {
                    spans.push(Span::raw("✎"));
                }
            } else {
                spans.push(Span::styled(
                    format!("{:<14} ", crate::model::truncate_cell(&agents, 14)),
                    style(Role::Muted),
                ));
                spans.push(state);
            }
            Entry {
                spans,
                target: Some(HitTarget::ListRow(index)),
                picked: index == screen.selected,
                heading: false,
            }
        })
        .collect();
    widgets::entries(frame, inner, model, hits, &entries);
}

/// The labels' column of the sheet.
const LABEL: usize = 12;

/// A section's title in the sheet, then the keys that act on its rows.
fn heading(model: &Model, title: &str, keys: &str) -> Line<'static> {
    let style = |role: Role| model.theme.style(role, model.capabilities);
    Line::from(vec![
        Span::styled(
            format!(" {title}"),
            style(Role::Border).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("   {keys}"), style(Role::Muted)),
    ])
}

/// A row of the sheet the keys walk: marked and reversed while they are on it, so it
/// reads without colour.
fn walked(spans: Vec<Span<'static>>, picked: bool, width: usize) -> Line<'static> {
    let mut all = vec![Span::raw(if picked { " > " } else { "   " })];
    all.extend(spans);
    let mut line = Line::from(all);
    if picked {
        let used = line.width();
        line.spans
            .push(Span::raw(" ".repeat(width.saturating_sub(used))));
        line = line.patch_style(Style::default().add_modifier(Modifier::REVERSED));
    }
    line
}

/// The picked profile on one page: its state, what it reads and sees, then its
/// connections, with what it may write on each, and its agents, both walked with the keys.
fn sheet(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let screen = &model.mcp_profiles;
    let Some(profile) = screen.selected_profile() else {
        return;
    };
    if area.width < 2 || area.height < 2 {
        return;
    }
    let focused = super::section(model) == super::Section::Detail;
    let block = crate::render::pane_block(model, &profile.name, focused);
    let mut inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenDetail, area);
    let used = widgets::action_bar(frame, inner, model, hits, &profile_buttons(model), None);
    let gap = u16::from(inner.height > used + 3);
    inner = Rect::new(
        inner.x,
        inner.y + used + gap,
        inner.width,
        inner.height.saturating_sub(used + gap),
    );
    let width = usize::from(inner.width);
    let style = |role: Role| model.theme.style(role, model.capabilities);
    let footer: Vec<String> = match &screen.confirm {
        Some(confirm) => confirm.lines(),
        // What was done, a line each, each wrapped to the pane.
        None => screen
            .status
            .lines()
            .flat_map(|line| crate::model::wrap_words(line, width.saturating_sub(1).max(8)))
            .map(|line| format!(" {line}"))
            .collect(),
    };
    let footer_rows = (footer.len() as u16).min(inner.height);
    let body = Rect::new(inner.x, inner.y, inner.width, inner.height - footer_rows);

    // (the walked row it is, the line)
    let mut lines: Vec<(Option<usize>, Line<'static>)> = Vec::new();
    let (glyph, role, state) = if profile.enabled {
        ("● ", Role::Success, "enabled: agents can connect")
    } else {
        ("○ ", Role::Muted, "disabled: agents cannot connect")
    };
    lines.push((
        None,
        Line::from(vec![
            Span::styled(format!(" {:<LABEL$}  ", "Status"), style(Role::Muted)),
            Span::styled(glyph, style(role)),
            Span::raw(state),
        ]),
    ));
    lines.extend(
        widgets::field_lines_with(
            model,
            &[
                FieldRow::Field("Reads", profile.reads_words().into()),
                FieldRow::Field("Sees", profile.sees_words()),
            ],
            inner.width,
            LABEL,
        )
        .into_iter()
        .map(|line| (None, line)),
    );
    lines.push((None, Line::default()));
    lines.push((
        None,
        heading(
            model,
            "CONNECTIONS",
            if profile.unasked() {
                "some writes run without asking · w writes · r revoke"
            } else {
                "each write asks you first · w writes · r revoke"
            },
        ),
    ));
    // The walk is marked while the keys are on it, not under a question.
    let current = if focused && screen.confirm.is_none() {
        screen.current(&model.mcp_clients)
    } else {
        None
    };
    let rows = screen.rows(&model.mcp_clients);
    let place = |row: Row| rows.iter().position(|at| *at == row);
    for (at, connection) in profile.connections.iter().enumerate() {
        let (writes, role) = match (&connection.access, &connection.refused) {
            (Some(access), None) => (
                format!("✎ {}", access.words(model.mcp_audit.now)),
                Role::Foreground,
            ),
            // Write access the server opens nothing with, said with the reason.
            (Some(access), Some(why)) => (
                format!("✎ {} -- {why}", access.words(model.mcp_audit.now)),
                Role::Warning,
            ),
            (None, Some(why)) if why.contains("production") => {
                ("read only: production".to_string(), Role::Muted)
            }
            (None, Some(_)) => ("read only: kept read-only".to_string(), Role::Muted),
            (None, None) => ("read only".to_string(), Role::Muted),
        };
        let environment =
            crate::accessibility::environment_role(&connection.environment).unwrap_or(Role::Muted);
        let room = width.saturating_sub(3 + 14 + 13);
        let row = Row::Connection(at);
        lines.push((
            place(row),
            walked(
                vec![
                    Span::raw(format!(
                        "{:<14}",
                        crate::model::truncate_cell(&connection.name, 13)
                    )),
                    Span::styled(
                        format!(
                            "{:<13}",
                            crate::model::truncate_cell(&connection.environment, 12)
                        ),
                        style(environment),
                    ),
                    Span::styled(crate::model::truncate_cell(&writes, room), style(role)),
                ],
                current == Some(row),
                width,
            ),
        ));
    }
    if profile.any {
        lines.push((
            None,
            Line::styled(
                "   it lists no connection yet, so no agent can use it: Edit picks them",
                style(Role::Muted),
            ),
        ));
    }
    lines.push((None, Line::default()));
    lines.push((
        None,
        heading(model, "AGENTS", "a connect · y copy its command"),
    ));
    let agents = model.mcp_clients.of(&profile.name);
    if agents.is_empty() {
        lines.push((
            None,
            Line::styled(
                "   none: a connects Claude Code, Codex, Cursor and the rest",
                style(Role::Muted),
            ),
        ));
    }
    for agent in agents {
        let row = Row::Agent(agent.client);
        let room = width.saturating_sub(3 + 2 + 16);
        lines.push((
            place(row),
            walked(
                vec![
                    Span::styled("✓ ", style(Role::Success)),
                    Span::raw(format!("{:<16}", agent.client.name())),
                    Span::styled(
                        crate::widgets::document_tabs::cut_middle(&agent.path, room.max(8)),
                        style(Role::Muted),
                    ),
                ],
                current == Some(row),
                width,
            ),
        ));
    }
    // Scrolled to keep the walked row in sight on a short pane.
    let height = usize::from(body.height);
    let walked_line = current
        .and_then(place)
        .and_then(|index| lines.iter().position(|(row, _)| *row == Some(index)))
        .unwrap_or(0);
    let offset = (walked_line + 1).saturating_sub(height);
    let shown: Vec<Line> = lines
        .iter()
        .skip(offset)
        .take(height)
        .map(|(_, line)| line.clone())
        .collect();
    frame.render_widget(Paragraph::new(shown), body);
    for (line, (row, _)) in lines.iter().enumerate().skip(offset).take(height) {
        if let Some(row) = row {
            hits.register(
                HitTarget::FormField(*row),
                crate::mouse::line_rect(body, line - offset),
            );
        }
    }
    let footer_area = Rect::new(inner.x, body.bottom(), inner.width, footer_rows);
    frame.render_widget(Paragraph::new(footer.join("\n")), footer_area);
    if let Some(confirm) = &screen.confirm {
        register_buttons(hits, footer_area, &footer, confirm.submit_label());
    }
}

fn draw_form(frame: &mut Frame, area: Rect, model: &Model, form: &Form, hits: &mut HitMap) {
    match form {
        Form::Profile(form) => profile_form(frame, area, model, form, hits),
        Form::Writes(form) => writes_form(frame, area, model, form, hits),
        Form::Agent(form) => agent_form(frame, area, model, form, hits),
    }
}

fn buttons<'a>(cancel: bool, submit: bool, label: &'a str) -> [panel::Button<'a>; 2] {
    [
        panel::Button {
            label: "Cancel",
            focused: cancel,
            tone: Role::Focus,
            primary: false,
            target: HitTarget::FooterCancel,
        },
        panel::Button {
            label,
            focused: submit,
            tone: Role::Focus,
            primary: true,
            target: HitTarget::FooterSubmit,
        },
    ]
}

/// An agent in a dropdown: its name, and where it stands.
fn agent_spans(
    model: &Model,
    agent: Option<&crate::screens::mcp_profiles::AgentChoice>,
) -> Vec<Span<'static>> {
    match agent {
        Some(agent) => vec![
            Span::styled(
                format!("{:<15}", agent.client.name()),
                panel::fg(model, Role::Foreground),
            ),
            Span::styled(agent.status.clone(), panel::fg(model, Role::Muted)),
        ],
        None => vec![
            Span::styled("None", panel::fg(model, Role::Foreground)),
            Span::styled("  connect one later with a", panel::fg(model, Role::Muted)),
        ],
    }
}

/// New profile, and Edit: what it uses, reads and sees; a new one, its agent too.
fn profile_form(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    form: &ProfileForm,
    hits: &mut HitMap,
) {
    let mut lines = Lines::new(panel::value_width(area));
    let at = |row: ProfileRow| form.index_of(row);
    let focused = |row: ProfileRow| form.focus == row;
    let invalid = |row: ProfileRow| form.error.as_ref().is_some_and(|(at, _)| *at == row);
    let muted = panel::fg(model, Role::Muted);
    if !form.editing {
        lines.section(model, "WHO");
        lines.text(
            model,
            at(ProfileRow::Name),
            "Name",
            &form.name,
            "",
            "letters, digits, - and _",
            focused(ProfileRow::Name),
            invalid(ProfileRow::Name),
        );
    }
    lines.section(model, "WHAT IT READS");
    let items: Vec<(String, bool)> = form
        .connections
        .iter()
        .map(|(choice, on)| (choice.name.clone(), *on))
        .collect();
    lines.grid(
        model,
        at(ProfileRow::Connections),
        "Connections",
        &[(String::new(), items)],
        focused(ProfileRow::Connections).then_some(form.conn_cursor),
    );
    for choice in form.picked_connections() {
        if let Some(why) = &choice.refused {
            lines.aside(
                format!("{}: agents read it, never write to it ({why})", choice.name),
                muted,
            );
        }
    }
    lines.segmented(
        model,
        at(ProfileRow::Reads),
        "Reads",
        &["Tables", "Tables and SELECT"],
        usize::from(form.sql),
        focused(ProfileRow::Reads),
        "",
    );
    lines.aside(
        if form.sql {
            "browse and describe, and one SELECT at a time, 1000 rows at most"
        } else {
            "browse, describe and read pages, no SQL"
        },
        muted,
    );
    lines.segmented(
        model,
        at(ProfileRow::Sees),
        "Sees",
        &["Everything", "Everything, except…", "Only…"],
        match form.sees {
            Sees::All => 0,
            Sees::Except => 1,
            Sees::Only => 2,
        },
        focused(ProfileRow::Sees),
        "",
    );
    lines.aside(
        match form.sees {
            Sees::All => "every table of its connections",
            Sees::Except => "the tables checked below are hidden from it",
            Sees::Only => "it sees only the tables checked below",
        },
        muted,
    );
    if form.sees != Sees::All {
        let groups: Vec<(String, Vec<(String, bool)>)> = form
            .table_groups()
            .into_iter()
            .map(|(title, items)| {
                (
                    title,
                    items
                        .into_iter()
                        .map(|(_, shown, on)| (shown, on))
                        .collect(),
                )
            })
            .collect();
        lines.grid(
            model,
            at(ProfileRow::Tables),
            "Tables",
            &groups,
            focused(ProfileRow::Tables).then_some(form.table_cursor),
        );
        for choice in form.picked_connections() {
            match form
                .tables
                .iter()
                .find(|(name, _)| *name == choice.name)
                .map(|(_, state)| state)
            {
                Some(TableState::NeverRead) => lines.aside(
                    format!(
                        "Dexo has not read {}'s tables: open it once in the Workbench",
                        choice.name
                    ),
                    panel::fg(model, Role::Warning),
                ),
                Some(TableState::Read(_)) => {}
                _ => lines.aside(format!("reading {}'s tables…", choice.name), muted),
            }
        }
        if !form.kept.is_empty() && form.sees == Sees::Only {
            lines.aside(
                format!(
                    "never {}: dexo mcp allow --remove takes it back",
                    form.kept.join(", ")
                ),
                muted,
            );
        }
    }
    if !form.editing {
        lines.section(model, "AGENT");
        let open = form
            .open
            .filter(|_| focused(ProfileRow::Agent))
            .map(|highlight| {
                (
                    std::iter::once(None)
                        .chain(form.agents.iter().map(Some))
                        .map(|agent| agent_spans(model, agent))
                        .collect(),
                    highlight,
                )
            });
        lines.dropdown(
            model,
            at(ProfileRow::Agent),
            "Connect",
            agent_spans(model, form.agent_choice()),
            focused(ProfileRow::Agent),
            invalid(ProfileRow::Agent),
            open,
        );
        if let Some(agent) = form.agent_choice() {
            lines.aside(format!("writes {}", agent.path), muted);
            if agent.skill {
                lines.toggle(
                    model,
                    at(ProfileRow::Skill),
                    "Skill file",
                    form.skill,
                    if form.skill {
                        "tells the agent how Dexo's tools behave"
                    } else {
                        "no skill file"
                    },
                    focused(ProfileRow::Skill),
                );
            }
        }
    }
    lines.error(
        model,
        form.error.as_ref().map(|(_, message)| message.as_str()),
    );

    lines.section(model, "RESULT");
    let name = form.name.trim().to_string();
    let connections: Vec<String> = form
        .picked_connections()
        .iter()
        .map(|choice| choice.name.clone())
        .collect();
    if connections.is_empty() {
        lines.note("check the connections it may use", muted);
    } else {
        lines.note(
            format!(
                "{name} reads {}: {}, {}.",
                connections.join(", "),
                if form.sql {
                    "tables and SELECT"
                } else {
                    "tables only"
                },
                crate::screens::mcp_profiles::sees_words(form.sees, &form.checked, &form.kept)
            ),
            panel::fg(model, Role::Foreground),
        );
    }
    if form.editing {
        // Unchecking a connection it may write to takes the writes back.
        if let Some(profile) = model
            .mcp_profiles
            .profiles
            .iter()
            .find(|profile| profile.name == name)
        {
            for line in &profile.connections {
                if line.access.is_some() && !connections.contains(&line.name) {
                    lines.note(
                        format!("! it may no longer write on {}", line.name),
                        panel::bold(model, Role::Warning),
                    );
                }
            }
        }
    } else {
        if let Some(agent) = form.agent_choice() {
            lines.note(
                format!(
                    "{} starts it, enabled, from {}.",
                    agent.client.name(),
                    agent.path
                ),
                panel::fg(model, Role::Foreground),
            );
            if let Some(other) = &agent.uses {
                lines.note(
                    format!("{} used {other} until now.", agent.client.name()),
                    panel::fg(model, Role::Warning),
                );
            }
        }
        lines.note(
            "It cannot write: w on a connection lets it, asking you each time.",
            muted,
        );
    }
    if form.busy {
        lines.note("… saving", muted);
    }
    if matches!(form.focus, ProfileRow::Cancel | ProfileRow::Save) {
        lines.focus_end();
    }
    let title = if form.editing {
        format!("Edit {name}")
    } else {
        "New profile".to_string()
    };
    let label = if form.editing {
        "Save"
    } else {
        "Create profile"
    };
    panel::draw(
        frame,
        area,
        model,
        hits,
        &title,
        lines,
        &buttons(
            focused(ProfileRow::Cancel),
            focused(ProfileRow::Save),
            label,
        ),
        form.hints(),
    );
}

/// A connection in Writes' dropdown: its name, its environment, whether it has writes.
fn connection_spans(
    model: &Model,
    line: &crate::screens::mcp_profiles::ConnectionLine,
) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        format!("{:<14}", line.name),
        panel::fg(model, Role::Foreground),
    )];
    if let Some(role) = crate::accessibility::environment_role(&line.environment) {
        spans.push(Span::styled(
            format!("{:<12}", line.environment),
            panel::fg(model, role),
        ));
    }
    if line.access.is_some() {
        spans.push(Span::styled("✎ has writes", panel::fg(model, Role::Muted)));
    }
    spans
}

/// Writes: on which connection, what it may change, for how long; every write asks.
fn writes_form(frame: &mut Frame, area: Rect, model: &Model, form: &WritesForm, hits: &mut HitMap) {
    let mut lines = Lines::new(panel::value_width(area));
    let at = |row: WritesRow| form.index_of(row);
    let focused = |row: WritesRow| form.focus == row;
    let invalid = |row: WritesRow| form.error.as_ref().is_some_and(|(at, _)| *at == row);
    let muted = panel::fg(model, Role::Muted);
    lines.section(model, "WHERE");
    let open = form
        .open
        .filter(|_| focused(WritesRow::Connection))
        .map(|highlight| {
            (
                form.offered
                    .iter()
                    .map(|line| connection_spans(model, line))
                    .collect(),
                highlight,
            )
        });
    let value = form
        .offered
        .get(form.connection)
        .map(|line| connection_spans(model, line))
        .unwrap_or_default();
    lines.dropdown(
        model,
        at(WritesRow::Connection),
        "Connection",
        value,
        focused(WritesRow::Connection),
        invalid(WritesRow::Connection),
        open,
    );
    for (name, why) in &form.left_out {
        lines.aside(format!("{name} is not offered: {why}"), muted);
    }
    lines.aside(
        format!("covers everything {} sees there", form.profile),
        muted,
    );
    lines.section(model, "WHAT");
    let items: Vec<(String, bool)> = form
        .kinds
        .iter()
        .map(|kind| {
            (
                crate::screens::mcp_profiles::kind_label(*kind)
                    .0
                    .to_string(),
                form.checked.contains(kind),
            )
        })
        .collect();
    lines.grid(
        model,
        at(WritesRow::Kinds),
        "May",
        &[(String::new(), items)],
        focused(WritesRow::Kinds).then_some(form.cursor),
    );
    if let Some(kind) = form.kinds.get(form.cursor) {
        let (label, covers) = crate::screens::mcp_profiles::kind_label(*kind);
        lines.aside(format!("{label}: {covers}"), muted);
    }
    let operations = form.operations();
    if !operations.is_empty() {
        use crate::screens::mcp_profiles::{PRESETS, operation_label};
        lines.section(model, "WITHOUT ASKING");
        let names: Vec<&str> = PRESETS.iter().map(|(name, _, _)| *name).collect();
        lines.segmented(
            model,
            at(WritesRow::Preset),
            "Preset",
            &names,
            form.preset().unwrap_or(usize::MAX),
            focused(WritesRow::Preset),
            "",
        );
        lines.aside(
            match form.preset() {
                Some(at) => PRESETS[at].2.to_string(),
                None => format!(
                    "{} run without asking",
                    dexo_app::mcp::bypass::operations_words(&form.bypass)
                ),
            },
            muted,
        );
        let items: Vec<(String, bool)> = operations
            .iter()
            .map(|operation| {
                (
                    operation_label(*operation).0.to_string(),
                    form.bypass.contains(operation),
                )
            })
            .collect();
        lines.grid(
            model,
            at(WritesRow::Bypass),
            "Runs",
            &[(String::new(), items)],
            focused(WritesRow::Bypass).then_some(form.bypass_cursor),
        );
        if focused(WritesRow::Bypass)
            && let Some(operation) = operations.get(form.bypass_cursor)
        {
            let (label, covers) = operation_label(*operation);
            lines.aside(format!("{label}: {covers}"), muted);
        }
    }
    lines.section(model, "HOW LONG");
    let options: Vec<&str> = crate::screens::mcp_profiles::LASTS
        .iter()
        .map(|(_, shown, _)| *shown)
        .collect();
    lines.segmented(
        model,
        at(WritesRow::Lasts),
        "For",
        &options,
        form.lasts,
        focused(WritesRow::Lasts),
        "",
    );
    let production = form.production();
    if production {
        lines.section(model, "CONFIRM");
        lines.confirm(
            model,
            at(WritesRow::Confirm),
            &form.connection_name(),
            &form.confirm,
            focused(WritesRow::Confirm),
            invalid(WritesRow::Confirm),
        );
    }
    lines.error(
        model,
        form.error.as_ref().map(|(_, message)| message.as_str()),
    );
    lines.section(model, "RESULT");
    match form.result() {
        Some(result) => lines.note(result, panel::fg(model, Role::Foreground)),
        None => lines.note("check what it may change", muted),
    }
    if form.bypass.len() < operations.len() {
        lines.note(
            format!(
                "A write that asks waits up to {} on Approvals, then is refused.",
                crate::screens::mcp_profiles::duration_words(i64::from(
                    dexo_app::mcp::approval::DEFAULT_TIMEOUT_SECS
                ))
            ),
            muted,
        );
    }
    // What it risks, in words, before it is given.
    if let Some(risk) = dexo_app::mcp::bypass::bypass_risk(&form.bypass, production) {
        let destructive = form.bypass.iter().any(|operation| operation.destructive());
        lines.note(
            format!("! {risk}"),
            panel::bold(
                model,
                if destructive {
                    Role::Error
                } else {
                    Role::Warning
                },
            ),
        );
    } else if production {
        lines.note(
            format!(
                "! {} is production: every write asks you first",
                form.connection_name()
            ),
            panel::bold(model, Role::Warning),
        );
    }
    if let Some(access) = form.current() {
        lines.note(
            format!(
                "Replaces what it has there now: {}.",
                access.words(model.mcp_audit.now)
            ),
            panel::fg(model, Role::Warning),
        );
    }
    if form.until_revoked() {
        lines.note(
            "! no time limit: r on the connection takes it back",
            panel::bold(model, Role::Warning),
        );
    }
    if form.busy {
        lines.note("… saving", muted);
    }
    if matches!(form.focus, WritesRow::Cancel | WritesRow::Create) {
        lines.focus_end();
    }
    let label = if form.current().is_some() {
        "Change writes"
    } else {
        "Allow writes"
    };
    let mut row = buttons(
        focused(WritesRow::Cancel),
        focused(WritesRow::Create),
        label,
    );
    if production {
        row[1].tone = Role::Warning;
    }
    panel::draw(
        frame,
        area,
        model,
        hits,
        &format!("Writes of {}", form.profile),
        lines,
        &row,
        form.hints(),
    );
}

/// Connect agent: which agent starts Dexo with this profile, with a skill file or not.
fn agent_form(frame: &mut Frame, area: Rect, model: &Model, form: &AgentForm, hits: &mut HitMap) {
    use crate::screens::mcp_profiles::AgentRow;
    let mut lines = Lines::new(panel::value_width(area));
    let at = |row: AgentRow| form.index_of(row);
    let focused = |row: AgentRow| form.focus == row;
    let muted = panel::fg(model, Role::Muted);
    let warning = panel::fg(model, Role::Warning);
    lines.section(model, "AGENT");
    let open = form
        .open
        .filter(|_| focused(AgentRow::Agent))
        .map(|highlight| {
            (
                form.agents
                    .iter()
                    .map(|agent| agent_spans(model, Some(agent)))
                    .collect(),
                highlight,
            )
        });
    lines.dropdown(
        model,
        at(AgentRow::Agent),
        "Agent",
        agent_spans(model, form.choice()),
        focused(AgentRow::Agent),
        form.error.is_some(),
        open,
    );
    if let Some(agent) = form.choice() {
        match &agent.unusable {
            Some(why) => lines.aside(
                format!("{} cannot be read, and is left as it is: {why}", agent.path),
                warning,
            ),
            None => lines.aside(
                format!("writes {}, the old one kept as a backup", agent.path),
                muted,
            ),
        }
        if !agent.found {
            lines.aside(
                "not found on this machine: the file is written for when it is",
                muted,
            );
        }
        if agent.skill {
            lines.toggle(
                model,
                at(AgentRow::Skill),
                "Skill file",
                form.skill,
                if form.skill {
                    "tells the agent how Dexo's tools behave"
                } else {
                    "no skill file"
                },
                focused(AgentRow::Skill),
            );
        }
    }
    lines.error(model, form.error.as_deref());
    lines.section(model, "RESULT");
    if let Some(agent) = form.choice() {
        lines.note(
            format!("{} starts Dexo with {}.", agent.client.name(), form.profile),
            panel::fg(model, Role::Foreground),
        );
        if let Some(other) = agent.uses.as_ref().filter(|uses| **uses != form.profile) {
            lines.note(format!("It started {other} until now."), warning);
        }
        if !form.enabled {
            lines.note(
                format!("{} is disabled: connecting enables it.", form.profile),
                warning,
            );
        }
        lines.note(
            format!("→ restart {} once it is written", agent.client.name()),
            muted,
        );
    }
    if form.busy {
        lines.note("… writing", muted);
    }
    if matches!(form.focus, AgentRow::Cancel | AgentRow::Connect) {
        lines.focus_end();
    }
    panel::draw(
        frame,
        area,
        model,
        hits,
        &format!("Connect an agent to {}", form.profile),
        lines,
        &buttons(
            focused(AgentRow::Cancel),
            focused(AgentRow::Connect),
            "Connect",
        ),
        form.hints(),
    );
}

/// The buttons of a question at the bottom of a pane.
fn register_buttons(hits: &mut HitMap, area: Rect, lines: &[String], submit: &str) {
    for (index, line) in lines.iter().enumerate().take(usize::from(area.height)) {
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(
                hits,
                crate::mouse::line_rect(area, index),
                line,
                submit,
            );
        }
    }
}

/// The picked profile, for a hint: whatever the detail walks.
fn walked_row(model: &Model) -> Option<Row> {
    (super::section(model) == super::Section::Detail)
        .then(|| model.mcp_profiles.current(&model.mcp_clients))
        .flatten()
}

pub fn hints(model: &Model) -> String {
    let views = "1-3 views";
    match model.agents_view {
        AgentsView::Approvals if model.mcp_audit.deciding.is_some() => {
            "Left/Right pick  Enter answer  Esc cancel".into()
        }
        AgentsView::Approvals => {
            format!("Up/Down pick  PgUp/PgDn read  {views}  Esc back")
        }
        AgentsView::Activity if model.mcp_audit.search.typing => {
            "Type to search  Up/Down pick  Enter keep  Esc clear".into()
        }
        AgentsView::Activity => {
            format!("Up/Down pick  / search  p profile  o outcome  {views}  Esc back")
        }
        // A form says its keys under its buttons.
        AgentsView::Profiles if model.mcp_profiles.form.is_some() => String::new(),
        AgentsView::Profiles if model.mcp_profiles.confirm.is_some() => {
            "Left/Right pick  Enter answer  Esc cancel".into()
        }
        AgentsView::Profiles => match walked_row(model) {
            Some(Row::Connection(_)) => {
                "Up/Down walk  w writes  r revoke  Enter edit  Esc the list".into()
            }
            Some(Row::Agent(_)) => {
                "Up/Down walk  a set up again  y copy its command  Esc the list".into()
            }
            None if super::section(model) == super::Section::Detail => {
                "a connect an agent  Esc the list".into()
            }
            None => {
                format!("Up/Down pick  Tab its details  Enter edit  w writes  a agent  {views}")
            }
        },
    }
}
