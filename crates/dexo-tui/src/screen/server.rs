//! Server: the sessions of the server a connection reaches, read again every two seconds
//! while on screen. It was Inspect Sessions, a dialog that cut every query at forty
//! characters on a terminal of a hundred and sixty.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::Paragraph;

use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};
use crate::screens::admin::AdminScreen;
use crate::theme::Role;

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let admin = &model.admin;
    if admin.server.is_none() {
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
    }
    let summary = Rect::new(area.x, area.y, area.width, 1.min(area.height));
    frame.render_widget(
        Paragraph::new(admin.summary()).style(model.theme.style(Role::Muted, model.capabilities)),
        summary,
    );
    let body = Rect::new(
        area.x,
        area.y + summary.height,
        area.width,
        area.height.saturating_sub(summary.height),
    );
    if admin.sessions.is_empty() {
        let mut lines = vec![if admin.loading {
            "Reading the server's sessions...".to_string()
        } else {
            "No sessions.".to_string()
        }];
        lines.extend(admin.last_error.clone());
        super::empty_state(frame, body, model, &lines);
        return;
    }
    let narrow = body.width < 100;
    let rows: Vec<String> = admin
        .sessions
        .iter()
        .map(|session| admin.row(session, narrow))
        .collect();
    let list_rows = (rows.len() as u16 + 3).clamp(5, (body.height * 3 / 5).max(5));
    let [list, detail] =
        Layout::vertical([Constraint::Length(list_rows), Constraint::Min(0)]).areas(body);
    super::list_pane(
        frame,
        list,
        model,
        hits,
        &format!("Sessions ({})", rows.len()),
        Some(&AdminScreen::header(narrow)),
        &rows,
        Some(admin.selected),
    );
    let width = usize::from(detail.width.saturating_sub(2));
    let lines = admin.detail_lines(width);
    let mut footer = Vec::new();
    if let Some(prompt) = &admin.terminate {
        footer.extend(prompt.lines());
    } else {
        footer.extend(admin.last_error.clone());
        footer.extend(admin.notice.clone());
    }
    let title = admin
        .picked()
        .map(|session| format!("Session {}", session.id))
        .unwrap_or_default();
    let (footer_area, max_scroll, page) = super::text_pane(
        frame,
        detail,
        model,
        hits,
        &title,
        &lines,
        usize::from(admin.detail_scroll),
        &footer,
    );
    hits.set_scroll_limit(crate::mouse::ScrollArea::Sessions, max_scroll);
    hits.set_page(crate::mouse::ScrollArea::Sessions, page);
    if let Some(prompt) = &admin.terminate {
        for (index, line) in footer
            .iter()
            .enumerate()
            .take(usize::from(footer_area.height))
        {
            let rect = crate::mouse::line_rect(footer_area, index);
            if line.starts_with("id:") {
                hits.register(HitTarget::FormField(0), rect);
                if prompt.footer == crate::widgets::form::FooterFocus::Input {
                    crate::render::show_input(frame, rect, "id: ", &prompt.typed, false);
                }
            } else if line.contains("[Cancel]") {
                crate::widgets::form::register_footer(hits, rect, line, "Terminate");
            }
        }
    }
}

pub fn hints(model: &Model) -> String {
    model.admin.keys()
}
