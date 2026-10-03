//! Agents: the writes agents wait to make, what they have done, and the profiles and
//! grants that let them. It was two dialogs, Agent Activity and MCP Profiles, the first a
//! box of seven rows on a terminal of forty-five.

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::Button;
use super::widgets::{self, Entry, FieldRow};
use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};
use crate::screens::mcp_profiles::McpProfilesScreen;
use crate::theme::Role;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AgentsView {
    #[default]
    Approvals,
    Activity,
    Profiles,
    /// Claude Code, Codex and the rest pointed at Dexo's MCP server.
    Setup,
}

impl AgentsView {
    pub const ALL: [AgentsView; 4] = [
        AgentsView::Approvals,
        AgentsView::Activity,
        AgentsView::Profiles,
        AgentsView::Setup,
    ];

    pub fn title(self) -> &'static str {
        match self {
            AgentsView::Approvals => "Approvals",
            AgentsView::Activity => "Activity",
            AgentsView::Profiles => "Profiles",
            AgentsView::Setup => "Setup",
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
        &toolbar_buttons(model),
    );
    match model.agents_view {
        AgentsView::Approvals => approvals(frame, body, model, hits),
        AgentsView::Activity => activity(frame, body, model, hits),
        AgentsView::Profiles => profiles(frame, body, model, hits),
        AgentsView::Setup => setup(frame, body, model, hits),
    }
}

/// The agents Dexo can be set up for, beside the form that sets the picked one up.
fn setup(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let setup = &model.mcp_setup;
    if setup.clients.is_empty() {
        super::empty_state(
            frame,
            area,
            model,
            &["Reading the agents' configs...".into()],
        );
        return;
    }
    // On a narrow screen the form, while it has the keys, takes it whole.
    if area.width < 100 && super::section(model) == super::Section::Detail {
        setup_form(frame, area, model, hits);
        return;
    }
    let (list, detail) = super::list_and_detail(area, setup.clients.len());
    setup_list(frame, list, model, hits);
    setup_form(frame, detail, model, hits);
}

/// Each agent with where it stands: set up and with which profile, found on this machine,
/// or not; one not found is dimmed and stays in its place.
fn setup_list(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    use dexo_app::mcp::clients::ClientState;
    if area.width < 2 || area.height < 2 {
        return;
    }
    let setup = &model.mcp_setup;
    let block = crate::render::pane_block(
        model,
        "Agents",
        super::section(model) == super::Section::List,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenList, area);
    let style = |role: Role| model.theme.style(role, model.capabilities);
    let entries: Vec<Entry> = setup
        .clients
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let (glyph, role) = match &row.state {
                ClientState::SetUp { .. } => ("✓", Role::Success),
                ClientState::Unusable(_) => ("!", Role::Error),
                _ => ("·", Role::Muted),
            };
            let name = format!(" {:<15} {}", row.client.name(), row.list_status());
            Entry {
                spans: vec![
                    Span::styled(glyph, style(role)),
                    if row.found || matches!(row.state, ClientState::SetUp { .. }) {
                        Span::raw(name)
                    } else {
                        Span::styled(name, style(Role::Muted))
                    },
                ],
                target: Some(HitTarget::ListRow(index)),
                picked: index == setup.selected,
                heading: false,
            }
        })
        .collect();
    widgets::entries(frame, inner, model, hits, &entries);
}

/// What can be done to the picked agent: set it up, or copy its own command for it.
pub fn setup_buttons(model: &Model) -> Vec<Button> {
    use dexo_app::mcp::clients::ClientState;
    let setup = &model.mcp_setup;
    let Some(row) = setup.current() else {
        return Vec::new();
    };
    let set_up = match &row.state {
        _ if setup.busy => {
            Button::new(KeyCode::Char('s'), "Set up").disabled("It is being set up.")
        }
        ClientState::Unusable(why) => Button::new(KeyCode::Char('s'), "Set up")
            .disabled(format!("{} is left as it is: {why}.", row.path)),
        _ => Button::new(KeyCode::Char('s'), "Set up"),
    };
    let command = row
        .client
        .by_hand(&setup.command, &setup.profile_name())
        .is_some();
    vec![
        set_up,
        Button::new(KeyCode::Char('y'), "Copy command").enabled_if(
            command,
            format!(
                "{} has no command of its own for it: s sets it up.",
                row.client.name()
            ),
        ),
    ]
}

/// The labels' column of the Setup detail, fields and form alike: as wide as Connections.
const LABEL: usize = 11;

fn setup_form(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    use crate::screens::mcp_setup::Row;
    use dexo_app::mcp::clients::ClientState;
    let setup = &model.mcp_setup;
    let Some(client) = setup.current() else {
        return;
    };
    if area.width < 2 || area.height < 2 {
        return;
    }
    let focused = super::section(model) == super::Section::Detail;
    let block = crate::render::pane_block(model, client.client.name(), focused);
    let mut inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenDetail, area);
    let used = widgets::action_bar(frame, inner, model, hits, &setup_buttons(model), None);
    let gap = u16::from(inner.height > used + 3);
    inner = Rect::new(
        inner.x,
        inner.y + used + gap,
        inner.width,
        inner.height.saturating_sub(used + gap),
    );
    let style = |role: Role| model.theme.style(role, model.capabilities);
    let width = inner.width;
    // (the form row it is, the line)
    let mut lines: Vec<(Option<usize>, Line)> = Vec::new();
    let status = match &client.state {
        ClientState::SetUp {
            profile: Some(profile),
            ..
        } => format!("set up · {profile}"),
        ClientState::SetUp { .. } => "set up".into(),
        ClientState::NoFile | ClientState::NotSetUp => "not set up".into(),
        ClientState::Unusable(why) => format!("its file cannot be read: {why}"),
    };
    let found = if client.found {
        "installed"
    } else {
        "not found on this machine"
    };
    let mut writes = client.path.clone();
    if client.client.per_project() {
        writes.push_str(" (this folder)");
    }
    writes.push_str(if matches!(client.state, ClientState::NoFile) {
        " · new file"
    } else {
        " · exists"
    });
    let mut fields = vec![
        FieldRow::Field("Status", format!("{status} · {found}")),
        FieldRow::Field("Writes", writes),
    ];
    if let Some(skill) = &client.skill {
        fields.push(FieldRow::Field("Skill", skill.clone()));
    }
    // The labels line up with the form's under them.
    lines.extend(
        widgets::field_lines_with(model, &fields, width, LABEL)
            .into_iter()
            .map(|line| (None, line)),
    );
    lines.push((None, Line::default()));
    let on = |yes: bool| if yes { "yes" } else { "no" };
    let muted = style(Role::Muted);
    for (index, row) in setup.rows().into_iter().enumerate() {
        let marker = if focused && setup.focused() == Some(row) {
            ">"
        } else {
            " "
        };
        let (label, value) = match row {
            Row::Profile => (
                "Profile",
                format!("< {} >", setup.profile.as_deref().unwrap_or("a new one")),
            ),
            Row::Name => ("Name", setup.name.as_str().to_string()),
            Row::Connection(at) => {
                let (name, checked) = &setup.connections[at];
                (
                    if at == 0 { "Connections" } else { "" },
                    format!("[{}] {name}", if *checked { "x" } else { " " }),
                )
            }
            Row::Reads => ("Read SQL", format!("< {} >", on(setup.reads))),
            Row::Skill => ("Skill file", format!("< {} >", on(setup.skill))),
        };
        lines.push((
            Some(index),
            Line::from(vec![
                Span::raw(marker),
                Span::styled(format!("{label:<LABEL$}  "), muted),
                Span::raw(value),
            ]),
        ));
        // An existing profile is used as it is; what it lets the agent do is said.
        if row == Row::Profile
            && let Some(name) = &setup.profile
            && let Some(found) = model
                .mcp_profiles
                .profiles
                .iter()
                .find(|profile| &profile.name == name)
        {
            let indent = 1 + LABEL + 2;
            let uses = if found.connections.is_empty() {
                "no connection yet".to_string()
            } else {
                found.connections.join(", ")
            };
            lines.push((
                None,
                Line::styled(
                    format!(
                        "{:indent$}uses {uses} · {} · {}",
                        "",
                        if found.raw_read { "read SQL" } else { "browse" },
                        if found.enabled { "enabled" } else { "disabled" }
                    ),
                    muted,
                ),
            ));
        }
    }
    // The form's own buttons, while it has the keys.
    let mut footer_row = None;
    if focused {
        lines.push((None, Line::default()));
        footer_row = Some(lines.len());
        lines.push((
            None,
            Line::raw(crate::widgets::form::footer_line("Set up", setup.footer)),
        ));
    }
    match (&setup.outcome, setup.busy) {
        (_, true) => {
            lines.push((None, Line::default()));
            lines.push((None, Line::styled("… setting it up", muted)));
        }
        (Some(Ok(done)), _) => {
            lines.push((None, Line::default()));
            for line in done {
                let role = if line.starts_with('✓') {
                    Role::Success
                } else {
                    Role::Warning
                };
                for part in crate::model::wrap_words(line, usize::from(width).max(8) - 1) {
                    lines.push((None, Line::styled(format!(" {part}"), style(role))));
                }
            }
        }
        (Some(Err(why)), _) => {
            lines.push((None, Line::default()));
            for part in crate::model::wrap_words(&format!("✗ {why}"), usize::from(width).max(8) - 1)
            {
                lines.push((None, Line::styled(format!(" {part}"), style(Role::Error))));
            }
        }
        (None, _) => {}
    }
    // Scrolled so the focused row, or the buttons, stay in sight.
    let height = usize::from(inner.height);
    let focus_line = if setup.footer == crate::widgets::form::FooterFocus::Input {
        lines
            .iter()
            .position(|(row, _)| *row == Some(setup.row))
            .unwrap_or(0)
    } else {
        footer_row.unwrap_or(0)
    };
    let offset = crate::palette::scroll_to_selection(focus_line, 0, lines.len(), height);
    let shown: Vec<Line> = lines
        .iter()
        .skip(offset)
        .take(height)
        .map(|(_, line)| line.clone())
        .collect();
    frame.render_widget(ratatui::widgets::Paragraph::new(shown), inner);
    for (line, (row, text)) in lines.iter().enumerate().skip(offset).take(height) {
        let rect = crate::mouse::line_rect(inner, line - offset);
        let text: String = text
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        if Some(line) == footer_row {
            crate::widgets::form::register_footer(hits, rect, &text, "Set up");
            continue;
        }
        let Some(row) = row else {
            continue;
        };
        hits.register(HitTarget::FormField(*row), rect);
        for (needle, step) in [("< ", -1), (" >", 1)] {
            crate::mouse::register_label(
                hits,
                rect,
                &text,
                needle,
                HitTarget::FormChoice { index: *row, step },
            );
        }
        if focused
            && setup.rows().get(*row) == Some(&Row::Name)
            && setup.focused() == Some(Row::Name)
        {
            let before = format!(">{:<LABEL$}  ", "Name");
            crate::render::show_input(frame, rect, &before, &setup.name, false);
        }
    }
}

/// What can be done to the picked request: approve it, or deny it.
pub fn approval_buttons(model: &Model) -> Vec<Button> {
    let audit = &model.mcp_audit;
    if audit.current().is_none() || audit.deciding.is_some() {
        return Vec::new();
    }
    vec![
        Button::new(KeyCode::Char('a'), "Approve"),
        Button::new(KeyCode::Char('d'), "Deny"),
    ]
}

fn approvals(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let audit = &model.mcp_audit;
    if audit.pending.is_empty() {
        let mut lines = vec![
            "No agent's write is waiting for approval.".to_string(),
            "An agent writing under a grant that asks first waits here for your answer."
                .to_string(),
        ];
        lines.extend(audit.notice.clone());
        super::empty_state(frame, area, model, &lines);
        return;
    }
    let rows: Vec<String> = audit
        .pending
        .iter()
        .map(|request| audit.row(request))
        .collect();
    let (list, detail) = super::list_and_detail(area, rows.len());
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
        text.extend(widgets::field_lines(
            model,
            &[
                FieldRow::Field("Profile", request.profile.clone()),
                FieldRow::Field("Tool", request.tool.clone()),
                FieldRow::Field("Connection", request.connection.clone()),
                FieldRow::Field("Target", request.targets.join(", ")),
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

/// The SQL dialect of the saved connection called `name`, for a statement's colours.
fn dialect_of(model: &Model, name: &str) -> dexo_sql::Dialect {
    let driver = model
        .connections
        .profiles
        .iter()
        .find(|row| row.profile.name == name)
        .map_or("postgres", |row| row.profile.driver.as_str());
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
    let detail_rows = 5.min(area.height / 3);
    let table = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(detail_rows),
    );
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
                ratatui::widgets::Paragraph::new(crate::model::truncate_cell(&header, width))
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
    if detail_rows >= 3 {
        let event = events[picked];
        let detail = Rect::new(area.x, table.bottom(), area.width, detail_rows);
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

/// What acts on the view as a whole: every grant taken back; a new profile.
pub fn toolbar_buttons(model: &Model) -> Vec<Button> {
    match model.agents_view {
        AgentsView::Approvals => vec![Button::new(KeyCode::Char('R'), "Revoke all grants")],
        AgentsView::Profiles => vec![Button::new(KeyCode::Char('n'), "New")],
        _ => Vec::new(),
    }
}

/// What can be done to the picked profile.
pub fn profile_buttons(model: &Model) -> Vec<Button> {
    let screen = &model.mcp_profiles;
    if screen.name.is_empty() || screen.grant_form.is_some() {
        return Vec::new();
    }
    vec![
        Button::new(
            KeyCode::Char('e'),
            if screen.enabled { "Disable" } else { "Enable" },
        ),
        Button::new(KeyCode::Char('c'), "Connections…"),
        Button::new(
            KeyCode::Char('q'),
            if screen.raw_read {
                "Read SQL off"
            } else {
                "Read SQL on"
            },
        ),
        Button::new(KeyCode::Char('g'), "Grant…"),
        Button::new(KeyCode::Char('r'), "Revoke grants").enabled_if(
            !screen.grants.is_empty(),
            format!("{} has no grant to revoke.", screen.name),
        ),
        Button::new(KeyCode::Char('x'), "Delete"),
    ]
}

fn profiles(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let screen = &model.mcp_profiles;
    if screen.profiles.is_empty() {
        if let Some(form) = &screen.grant_form {
            grant_form(frame, area, model, form, hits);
            return;
        }
        let mut lines: Vec<String> = McpProfilesScreen::EMPTY
            .iter()
            .map(|line| line.trim().to_string())
            .collect();
        if !screen.status.is_empty() {
            lines.push(String::new());
            lines.push(screen.status.clone());
        }
        super::empty_state(frame, area, model, &lines);
        return;
    }
    let (list, detail) = super::list_and_detail(area, screen.profiles.len());
    profiles_list(frame, list, model, hits);
    if let Some(form) = &screen.grant_form {
        grant_form(frame, detail, model, form, hits);
        return;
    }
    let width = detail.width.saturating_sub(2);
    let mut footer = Vec::new();
    if let Some(confirm) = &screen.confirm {
        footer.extend(confirm.lines(&screen.connections));
    } else if !screen.status.is_empty() {
        footer.extend(crate::model::wrap_words(
            &screen.status,
            usize::from(width).max(8),
        ));
    }
    let text = match &screen.checklist {
        Some(checklist) => checklist_lines(model, checklist),
        None => widgets::field_lines(model, &profile_fields(screen), width),
    };
    let drawn = super::detail_pane(
        frame,
        detail,
        model,
        hits,
        &screen.name,
        &profile_buttons(model),
        ratatui::text::Text::from(text),
        screen.detail_scroll,
        &footer,
    );
    hits.set_scroll_limit(crate::mouse::ScrollArea::McpProfiles, drawn.max_scroll);
    hits.set_page(crate::mouse::ScrollArea::McpProfiles, drawn.page);
    // A click on a connection checks it.
    if let Some(checklist) = &screen.checklist {
        let top = screen.detail_scroll.min(drawn.max_scroll);
        for index in 0..checklist.items.len() {
            let Some(line) = (index + 1).checked_sub(top) else {
                continue;
            };
            if line < usize::from(drawn.body.height) {
                hits.register(
                    HitTarget::FormField(index),
                    crate::mouse::line_rect(drawn.body, line),
                );
            }
        }
    }
    if let Some(confirm) = &screen.confirm {
        register_buttons(hits, drawn.footer, &footer, confirm.submit_label());
    }
}

/// Each profile with its state, its connections, what it reads and its grants.
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
    let style = |role: Role| model.theme.style(role, model.capabilities);
    let name_width = screen
        .profiles
        .iter()
        .map(|profile| profile.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(24);
    let entries: Vec<Entry> = screen
        .profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| Entry {
            spans: vec![
                if profile.enabled {
                    Span::styled("●", style(Role::Success))
                } else {
                    Span::styled("○", style(Role::Muted))
                },
                Span::raw(format!(
                    " {:<name_width$}  {}",
                    crate::model::truncate_cell(&profile.name, name_width),
                    McpProfilesScreen::columns(profile)
                )),
            ],
            target: Some(HitTarget::ListRow(index)),
            picked: index == screen.selected,
            heading: false,
        })
        .collect();
    widgets::entries(frame, inner, model, hits, &entries);
}

/// The picked profile in fields: what it may use, read and see, then its grants.
fn profile_fields(screen: &McpProfilesScreen) -> Vec<FieldRow> {
    let mut rows = vec![
        FieldRow::Field(
            "State",
            if screen.enabled {
                "enabled: agents can use it".into()
            } else {
                "disabled".into()
            },
        ),
        FieldRow::Field(
            "Connections",
            if screen.connections.is_empty() {
                "any".into()
            } else {
                screen.connections.join(", ")
            },
        ),
        FieldRow::Field(
            "Access",
            if screen.raw_read {
                "read SQL (one SELECT at a time, at most 1000 rows)".into()
            } else {
                "browse and describe only".into()
            },
        ),
    ];
    let (mut sees, mut denies) = (Vec::new(), Vec::new());
    for scope in &screen.scopes {
        match scope.strip_prefix("deny ") {
            Some(denied) => denies.push(denied.to_string()),
            None => sees.push(scope.strip_prefix("allow ").unwrap_or(scope).to_string()),
        }
    }
    rows.push(FieldRow::Field(
        "Sees",
        if sees.is_empty() {
            "nothing yet".into()
        } else {
            sees.join(", ")
        },
    ));
    if !denies.is_empty() {
        rows.push(FieldRow::Field("Never", denies.join(", ")));
    }
    if !screen.tools.is_empty() {
        rows.push(FieldRow::Field("Tools", screen.tools.join(", ")));
    }
    rows.push(FieldRow::Section("Grants"));
    if screen.grants.is_empty() {
        rows.push(FieldRow::Text("none: agents cannot write".into()));
    }
    for grant in &screen.grants {
        rows.push(FieldRow::Text(grant.words()));
    }
    rows
}

/// The connections the profile may use, checked, with the focus marked.
fn checklist_lines(
    model: &Model,
    checklist: &crate::screens::mcp_profiles::Checklist,
) -> Vec<Line<'static>> {
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let mut lines = vec![Line::styled(" Connections the agent may use", muted)];
    for (index, (name, on)) in checklist.items.iter().enumerate() {
        let marker = if index == checklist.row { ">" } else { " " };
        lines.push(Line::raw(format!(
            "{marker}[{}] {name}",
            if *on { "x" } else { " " }
        )));
    }
    lines
}

/// New grant, in the pane the profile's details were in.
fn grant_form(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    form: &crate::screens::mcp_profiles::GrantForm,
    hits: &mut HitMap,
) {
    let lines = form.lines();
    let block = crate::render::pane_block(model, "New grant", true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenDetail, area);
    frame.render_widget(ratatui::widgets::Paragraph::new(lines.join("\n")), inner);
    // The rows drawn are the fields that are shown, from the second line.
    let shown: Vec<usize> = (0..form.fields.len())
        .filter(|index| *index != crate::screens::mcp_profiles::GRANT_ASK_SECS || form.ask)
        .collect();
    for (index, line) in lines.iter().enumerate().take(usize::from(inner.height)) {
        let rect = crate::mouse::line_rect(inner, index);
        if let Some(field) = index.checked_sub(1).and_then(|row| shown.get(row)).copied() {
            if field == form.focus
                && !form.is_choice(field)
                && field != crate::screens::mcp_profiles::GRANT_ASK
            {
                crate::render::show_form_field(frame, rect, &form.fields[field]);
            }
            hits.register(HitTarget::FormField(field), rect);
            if form.is_choice(field) {
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
        } else if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Create");
        }
    }
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

pub fn hints(model: &Model) -> String {
    let views = "1-4 views";
    match model.agents_view {
        // What the focused row is for, where the form's sentences were.
        AgentsView::Setup if super::section(model) == super::Section::Detail => {
            match model.mcp_setup.focused() {
                Some(row) => format!("{}  Esc the list", crate::screens::mcp_setup::row_hint(row)),
                None => "Left/Right pick  Enter answer  Esc the list".into(),
            }
        }
        AgentsView::Setup => {
            format!("Up/Down pick  Enter the form  s set up  y copy the command  {views}  Esc back")
        }
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
        AgentsView::Profiles if model.mcp_profiles.grant_form.is_some() => {
            "Tab next  Left/Right change  Enter create  Esc cancel".into()
        }
        AgentsView::Profiles if model.mcp_profiles.confirm.is_some() => {
            "Left/Right pick  Enter answer  Esc cancel".into()
        }
        AgentsView::Profiles if model.mcp_profiles.checklist.is_some() => {
            "Up/Down move  Space check  Enter save  Esc keep them".into()
        }
        AgentsView::Profiles => {
            format!("Up/Down pick  R revoke all  PgUp/PgDn read  {views}  Esc back")
        }
    }
}
