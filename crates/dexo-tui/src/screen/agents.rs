//! Agents: the writes agents wait to make, what they have done, and the profiles and
//! grants that let them. It was two dialogs, Agent Activity and MCP Profiles, the first a
//! box of seven rows on a terminal of forty-five.

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};
use crate::screens::mcp_profiles::McpProfilesScreen;

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
    let body = super::views_bar(frame, area, model, hits, &views);
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
    let rows: Vec<String> = setup
        .clients
        .iter()
        .map(|row| format!("{:<15} {}", row.client.name(), row.status()))
        .collect();
    let (list, detail) = super::list_and_detail(area, rows.len());
    super::list_pane(
        frame,
        list,
        model,
        hits,
        "Agents",
        None,
        &rows,
        Some(setup.selected),
    );
    setup_form(frame, detail, model, hits);
}

fn setup_form(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    use crate::screens::mcp_setup::{Row, row_hint};
    use dexo_app::mcp::clients::ClientState;
    let setup = &model.mcp_setup;
    let Some(client) = setup.current() else {
        return;
    };
    if area.width < 2 || area.height < 2 {
        return;
    }
    let focused = super::section(model) == super::Section::Detail;
    let block =
        crate::render::pane_block(model, &format!("Set up {}", client.client.name()), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenDetail, area);
    let width = usize::from(inner.width).max(8);
    let wrap = |text: &str| crate::model::wrap_words(text, width);
    // (the form row it is, its text)
    let mut lines: Vec<(Option<usize>, String)> = Vec::new();
    let say = |lines: &mut Vec<(Option<usize>, String)>, text: &str| {
        lines.extend(wrap(text).into_iter().map(|line| (None, line)));
    };
    let place = if client.client.per_project() {
        format!(
            "Writes Dexo's server into {} -- this project's, the folder Dexo started in -- and keeps the old file beside it.",
            client.path
        )
    } else {
        format!(
            "Writes Dexo's server into {}, and keeps the old file beside it.",
            client.path
        )
    };
    say(&mut lines, &place);
    say(
        &mut lines,
        &match &client.state {
            ClientState::SetUp {
                profile: Some(profile),
                ..
            } => format!("Set up now, with {profile}."),
            ClientState::SetUp { .. } => "Set up now.".into(),
            ClientState::NoFile | ClientState::NotSetUp => "Not set up yet.".into(),
            ClientState::Unusable(why) => format!("Its file is left alone: {why}."),
        },
    );
    lines.push((None, String::new()));
    let on = |yes: bool| if yes { "yes" } else { "no" };
    for (index, row) in setup.rows().into_iter().enumerate() {
        let marker = if focused && setup.focused() == Some(row) {
            ">"
        } else {
            " "
        };
        let text = match row {
            Row::Profile => format!(
                "profile:      < {} >",
                setup.profile.as_deref().unwrap_or("a new one")
            ),
            Row::Name => format!("name:         {}", setup.name.as_str()),
            Row::Connection(at) => {
                let (name, checked) = &setup.connections[at];
                let label = if at == 0 { "connections:" } else { "" };
                format!("{label:<14}[{}] {name}", if *checked { "x" } else { " " })
            }
            Row::Reads => format!("read SQL:     < {} >", on(setup.reads)),
            Row::Skill => format!(
                "skill file:   < {} >  {}",
                on(setup.skill),
                client.skill.as_deref().unwrap_or_default()
            ),
        };
        lines.push((Some(index), format!("{marker} {text}")));
    }
    // An existing profile is used as it is; what it lets the agent do is said.
    if let Some(name) = &setup.profile
        && let Some(found) = model
            .mcp_profiles
            .profiles
            .iter()
            .find(|profile| &profile.name == name)
    {
        let uses = if found.connections.is_empty() {
            "no connection yet".to_string()
        } else {
            found.connections.join(", ")
        };
        let state = if found.enabled {
            "enabled"
        } else {
            "disabled: Set up enables it"
        };
        say(
            &mut lines,
            &format!(
                "  uses {uses}; {}; {state}",
                if found.raw_read {
                    "reads SQL"
                } else {
                    "browses only"
                }
            ),
        );
    }
    lines.push((None, String::new()));
    let footer_row = lines.len();
    let footer = crate::widgets::form::footer_line(
        "Set up",
        if focused {
            setup.footer
        } else {
            crate::widgets::form::FooterFocus::Input
        },
    );
    lines.push((None, footer.clone()));
    lines.push((None, String::new()));
    match (&setup.outcome, setup.busy) {
        (_, true) => say(&mut lines, "Setting it up..."),
        (Some(Ok(done)), _) => {
            for line in done {
                say(&mut lines, line);
            }
        }
        (Some(Err(why)), _) => say(&mut lines, why),
        (None, _) => {
            if let Some(row) = setup.focused().filter(|_| focused) {
                say(&mut lines, row_hint(row));
            }
        }
    }
    if let Some(command) = client.client.by_hand(&setup.command, &setup.profile_name()) {
        lines.push((None, String::new()));
        say(&mut lines, &format!("By hand: {command}"));
    }
    let shown: Vec<String> = lines
        .iter()
        .take(usize::from(inner.height))
        .map(|(_, line)| crate::model::truncate_cell(line, width))
        .collect();
    frame.render_widget(ratatui::widgets::Paragraph::new(shown.join("\n")), inner);
    for (line, (row, text)) in lines.iter().enumerate().take(usize::from(inner.height)) {
        let rect = crate::mouse::line_rect(inner, line);
        if line == footer_row {
            crate::widgets::form::register_footer(hits, rect, text, "Set up");
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
                text,
                needle,
                HitTarget::FormChoice { index: *row, step },
            );
        }
        if focused
            && setup.rows().get(*row) == Some(&Row::Name)
            && setup.focused() == Some(Row::Name)
        {
            crate::render::show_input(frame, rect, "> name:         ", &setup.name, false);
        }
    }
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
    let width = usize::from(detail.width.saturating_sub(2));
    let lines = audit.request_lines(width);
    let footer = audit.decision_lines(width);
    let (footer_area, max_scroll, page) = super::text_pane(
        frame,
        detail,
        model,
        hits,
        "Request",
        &lines,
        usize::from(audit.scroll),
        &footer,
    );
    hits.set_scroll_limit(crate::mouse::ScrollArea::McpAudit, max_scroll);
    hits.set_page(crate::mouse::ScrollArea::McpAudit, page);
    if let Some(deciding) = &audit.deciding {
        let label = if deciding.approve { "Approve" } else { "Deny" };
        register_buttons(hits, footer_area, &footer, label);
    }
}

fn activity(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let audit = &model.mcp_audit;
    let mut area = area;
    if audit.filtering || !audit.filter.is_empty() {
        let row = Rect::new(area.x, area.y, area.width, 1.min(area.height));
        let before = "Filter: ";
        frame.render_widget(
            ratatui::widgets::Paragraph::new(format!("{before}{}", audit.filter.as_str())),
            row,
        );
        if audit.filtering {
            crate::render::show_input(frame, row, before, &audit.filter, false);
        }
        area = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        );
    }
    let events = audit.visible_events();
    if events.is_empty() {
        let lines = if audit.events.is_empty() {
            vec![
                "No calls yet.".to_string(),
                "What agents do through Dexo is listed here as it happens.".to_string(),
            ]
        } else {
            vec![format!("No call matches \"{}\".", audit.filter.as_str())]
        };
        super::empty_state(frame, area, model, &lines);
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
    let inner_width = usize::from(table.width.saturating_sub(2));
    let target_width = inner_width
        .saturating_sub(8 + 2 + 12 + 2 + 20 + 2 + 24 + 2 + 7)
        .max(8);
    let header = format!(
        "{:<8}  {:<12}  {:<20}  {:<target_width$}  {:<24}  {:>7}",
        "TIME", "PROFILE", "TOOL", "TARGET", "OUTCOME", "MS"
    );
    let rows: Vec<String> = events
        .iter()
        .map(|event| {
            format!(
                "{:<8}  {:<12}  {:<20}  {:<target_width$}  {:<24}  {:>7}",
                event.time,
                crate::model::truncate_cell(&event.profile, 12),
                crate::model::truncate_cell(&event.tool, 20),
                crate::model::truncate_cell(&event.target, target_width),
                crate::model::truncate_cell(&event.outcome, 24),
                event.duration_ms
            )
        })
        .collect();
    super::list_pane(
        frame,
        table,
        model,
        hits,
        &format!("Recent calls ({})", events.len()),
        Some(&header),
        &rows,
        Some(picked),
    );
    if detail_rows >= 3 {
        let event = events[picked];
        let detail = Rect::new(area.x, table.bottom(), area.width, detail_rows);
        let lines = vec![
            format!("{} on {}", event.tool, event.target),
            format!(
                "by {} through {} at {}",
                event.profile, event.client, event.time
            ),
            format!(
                "{} · {} ms · {} row{}",
                event.outcome,
                event.duration_ms,
                event.rows,
                if event.rows == 1 { "" } else { "s" }
            ),
        ];
        super::text_pane(
            frame,
            detail,
            model,
            hits,
            "Call",
            &lines,
            usize::from(super::detail_scroll(model)),
            &[],
        );
    }
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
    let rows: Vec<String> = screen.profiles.iter().map(McpProfilesScreen::row).collect();
    let (list, detail) = super::list_and_detail(area, rows.len());
    super::list_pane(
        frame,
        list,
        model,
        hits,
        &format!("Profiles ({})", rows.len()),
        None,
        &rows,
        Some(screen.selected),
    );
    if let Some(form) = &screen.grant_form {
        grant_form(frame, detail, model, form, hits);
        return;
    }
    let width = usize::from(detail.width.saturating_sub(2));
    let lines = screen.detail_lines(width);
    let mut footer = Vec::new();
    if let Some(confirm) = &screen.confirm {
        footer.extend(confirm.lines(&screen.connections));
    } else if !screen.status.is_empty() {
        footer.extend(crate::model::wrap_words(&screen.status, width.max(8)));
    }
    let (footer_area, max_scroll, page) = super::text_pane(
        frame,
        detail,
        model,
        hits,
        &screen.name,
        &lines,
        screen.detail_scroll,
        &footer,
    );
    hits.set_scroll_limit(crate::mouse::ScrollArea::McpProfiles, max_scroll);
    hits.set_page(crate::mouse::ScrollArea::McpProfiles, page);
    if let Some(confirm) = &screen.confirm {
        register_buttons(hits, footer_area, &footer, confirm.submit_label());
    }
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
        AgentsView::Setup if super::section(model) == super::Section::Detail => {
            "Up/Down move  Left/Right/Space change  Enter next  Esc the list".into()
        }
        AgentsView::Setup => {
            format!("Up/Down pick  Enter set it up  c copy the command  {views}  Esc back")
        }
        AgentsView::Approvals if model.mcp_audit.deciding.is_some() => {
            "Left/Right pick  Enter answer  Esc cancel".into()
        }
        AgentsView::Approvals => {
            format!("a approve  d deny  Up/Down pick  PgUp/PgDn read  {views}  Esc back")
        }
        AgentsView::Activity if model.mcp_audit.filtering => {
            "type to filter  Enter keep  Esc clear".into()
        }
        AgentsView::Activity => format!("/ filter  Up/Down pick  {views}  Esc back"),
        AgentsView::Profiles if model.mcp_profiles.grant_form.is_some() => {
            "Tab next  Left/Right change  Enter create  Esc cancel".into()
        }
        AgentsView::Profiles if model.mcp_profiles.confirm.is_some() => {
            "Left/Right pick  Enter answer  Esc cancel".into()
        }
        AgentsView::Profiles => format!(
            "n new  e enable  g grant  r revoke  R revoke all  x delete  PgUp/PgDn read  {views}  Esc back"
        ),
    }
}
