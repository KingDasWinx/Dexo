//! Agents is a screen of its own: the header and the status line stay where they are,
//! and the calls get the room between them.
use dexo_tui::screens::mcp_audit::AuditLine;
use dexo_tui::{Action, Model, render::render_to_string, update};

#[test]
fn the_screen_keeps_the_header_and_the_status_line() {
    let mut model = Model::default();
    model.apply_size(80, 24);
    update(&mut model, Action::OpenMcpAudit);
    model.agents_view = dexo_tui::screen::agents::AgentsView::Activity;
    model.mcp_audit.events = (0..60)
        .map(|n| AuditLine {
            time: "12:00:00".into(),
            profile: "pg-dev".into(),
            tool: format!("event_{n}"),
            outcome: "ok".into(),
            ..AuditLine::default()
        })
        .collect();
    let screen = render_to_string(&model, 80, 24);
    let rows: Vec<&str> = screen.lines().collect();
    assert!(rows[0].contains("[Agents]"), "{screen}");
    assert!(rows[23].starts_with("Agents"), "{screen}");
    assert!(screen.contains("Recent calls (60)"), "{screen}");
    assert!(screen.contains("event_0"), "{screen}");
}
