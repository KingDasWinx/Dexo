//! Approvals answers by button, and Activity is filtered by profile and outcome as well
//! as by text.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::screen::agents::AgentsView;
use dexo_tui::screens::mcp_audit::AuditLine;
use dexo_tui::{Action, Effect, Model, update};

fn paint(model: &mut Model) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
    dexo_tui::render::render_to_string(model, 140, 40)
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    let effects = update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    paint(model);
    effects
}

fn approvals() -> Model {
    let request = dexo_app::mcp::Approval::pending(
        "assistant",
        "local",
        "data_execute_sql",
        serde_json::json!({"sql": "DELETE FROM orders WHERE id = 7"})
            .as_object()
            .unwrap(),
        vec!["db.public.orders".into()],
        1000,
        120,
    );
    let mut model = Model::default();
    update(&mut model, Action::OpenMcpAudit);
    update(
        &mut model,
        Action::McpAuditLoaded {
            events: Vec::new(),
            pending: vec![request],
            now: 1010,
        },
    );
    model.agents_view = AgentsView::Approvals;
    paint(&mut model);
    model
}

#[test]
fn a_waiting_write_is_answered_by_its_buttons() {
    let mut model = approvals();
    let frame = paint(&mut model);
    for text in [
        "[a Approve]",
        "[d Deny]",
        "[R Revoke all grants]",
        " Profile ",
        " Tool ",
        " Connection ",
        " Left ",
        "DELETE FROM orders WHERE id = 7",
    ] {
        assert!(frame.contains(text), "{text}: {frame}");
    }
    assert!(!frame.contains("a approve  d deny"), "{frame}");
    let (column, row) = model
        .hits
        .center(HitTarget::Press(KeyCode::Char('a'), false));
    update(
        &mut model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
    let deciding = model.mcp_audit.deciding.as_ref().expect("asks first");
    assert!(deciding.approve);
}

fn call(profile: &str, tool: &str, outcome: &str) -> AuditLine {
    AuditLine {
        time: "12:00:00".into(),
        profile: profile.into(),
        tool: tool.into(),
        outcome: outcome.into(),
        duration_ms: 3,
        ..AuditLine::default()
    }
}

fn activity() -> Model {
    let mut model = Model::default();
    update(&mut model, Action::OpenMcpAudit);
    model.mcp_audit.events = vec![
        call("assistant", "catalog_search", "ok"),
        call("assistant", "data_update", "refused: POLICY DENIED"),
        call("reviewer", "query_execute_read", "syntax error at or near"),
        call("reviewer", "object_describe", "ok"),
    ];
    model.agents_view = AgentsView::Activity;
    paint(&mut model);
    model
}

fn tools(model: &Model) -> Vec<String> {
    model
        .mcp_audit
        .visible_events()
        .iter()
        .map(|event| event.tool.clone())
        .collect()
}

#[test]
fn activity_is_filtered_by_profile_and_by_outcome() {
    let mut model = activity();
    let frame = paint(&mut model);
    assert!(frame.contains("[p Profile: all]"), "{frame}");
    assert!(frame.contains("[o Outcome: all]"), "{frame}");
    press(&mut model, KeyCode::Char('p'));
    assert_eq!(tools(&model), ["catalog_search", "data_update"]);
    press(&mut model, KeyCode::Char('p'));
    assert_eq!(tools(&model), ["query_execute_read", "object_describe"]);
    press(&mut model, KeyCode::Char('p'));
    assert_eq!(tools(&model).len(), 4);
    press(&mut model, KeyCode::Char('o'));
    assert_eq!(tools(&model), ["catalog_search", "object_describe"]);
    press(&mut model, KeyCode::Char('o'));
    assert_eq!(tools(&model), ["query_execute_read"]);
    press(&mut model, KeyCode::Char('o'));
    assert_eq!(tools(&model), ["data_update"]);
    let frame = paint(&mut model);
    assert!(frame.contains("Outcome: denied"), "{frame}");
    assert!(frame.contains("1 shown"), "{frame}");
    assert!(frame.contains("⊘"), "{frame}");
}

#[test]
fn activity_searches_after_slash() {
    let mut model = activity();
    press(&mut model, KeyCode::Char('/'));
    for ch in "describe".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    assert_eq!(tools(&model), ["object_describe"]);
    press(&mut model, KeyCode::Esc);
    assert_eq!(tools(&model).len(), 4);
}

/// Nothing shown, the Clear filters button clears the search and the filters at once.
#[test]
fn clear_filters_clears_everything_when_nothing_is_shown() {
    let mut model = activity();
    press(&mut model, KeyCode::Char('p'));
    model.mcp_audit.search.input.set_text("nothing like it");
    assert!(tools(&model).is_empty());
    let frame = paint(&mut model);
    assert!(frame.contains("[Esc Clear filters]"), "{frame}");
    press(&mut model, KeyCode::Esc);
    assert_eq!(tools(&model).len(), 4);
    assert_eq!(model.screen, dexo_tui::model::Screen::Agents);
}
