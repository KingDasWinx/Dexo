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
    let body = super::views_bar(frame, area, model, hits, &views);
    match model.agents_view {
        AgentsView::Approvals => approvals(frame, body, model, hits),
        AgentsView::Activity => activity(frame, body, model, hits),
        AgentsView::Profiles => profiles(frame, body, model, hits),
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
        super::text_pane(frame, detail, model, "Call", &lines, 0, &[]);
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
    let views = "1-3 views";
    match model.agents_view {
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
            "e enable  g grant  r revoke  R revoke all  x delete  PgUp/PgDn read  {views}  Esc back"
        ),
    }
}
