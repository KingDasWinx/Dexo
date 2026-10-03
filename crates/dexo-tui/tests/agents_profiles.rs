//! A profile's connections and whether it reads SQL are changed on Agents; they took
//! `dexo mcp profile set` at a shell.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_tui::screen::agents::AgentsView;
use dexo_tui::screens::mcp_profiles::McpProfileSummary;
use dexo_tui::{Action, Effect, Model, update};

fn connection(name: &str, driver: &str, n: u128) -> ConnectionProfile {
    ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(n)),
        None,
        name,
        driver,
        "local",
        serde_json::json!({"host":"h","port":5432,"username":"u","database":"d"}),
        SecretRef::new(format!("r{n}")),
    )
}

fn profiles() -> Model {
    let mut model = Model::default();
    model.connections.load_profiles(vec![
        connection("pg-dev", "postgres", 1),
        connection("my-dev", "mysql", 2),
        connection("shop", "sqlite", 3),
    ]);
    update(
        &mut model,
        Action::GoToScreen(dexo_tui::model::Screen::Agents),
    );
    update(
        &mut model,
        Action::McpProfilesLoaded {
            profiles: vec![McpProfileSummary {
                name: "assistant".into(),
                enabled: true,
                connections: vec!["pg-dev".into()],
                raw_read: true,
                scopes: vec!["allow *".into()],
                ..McpProfileSummary::default()
            }],
        },
    );
    model.agents_view = AgentsView::Profiles;
    paint(&mut model);
    model
}

fn paint(model: &mut Model) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
    let mut hits = dexo_tui::mouse::HitMap::default();
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

#[test]
fn the_profiles_buttons_are_over_its_fields() {
    let mut model = profiles();
    let frame = paint(&mut model);
    for text in [
        "[e Disable]",
        "[c Connections…]",
        "[q Read SQL off]",
        "[g Grant…]",
        "[x Delete]",
        "[n New]",
        " Connections ",
        " Access ",
        "read SQL",
    ] {
        assert!(frame.contains(text), "{text}: {frame}");
    }
    assert!(frame.contains("● assistant"), "{frame}");
}

#[test]
fn c_checks_the_connections_and_enter_saves_them() {
    let mut model = profiles();
    press(&mut model, KeyCode::Char('c'));
    let frame = paint(&mut model);
    assert!(frame.contains("[x] pg-dev"), "{frame}");
    assert!(frame.contains("[ ] my-dev"), "{frame}");
    assert!(
        !frame.contains("shop"),
        "MCP serves Postgres and MySQL: {frame}"
    );
    // my-dev is listed first, by name.
    press(&mut model, KeyCode::Char(' '));
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::SaveMcpProfileAccess { name, connections: Some(connections), reads: None }
                if name == "assistant" && connections == &["my-dev".to_string(), "pg-dev".to_string()]
        )),
        "{effects:?}"
    );
    assert!(model.mcp_profiles.checklist.is_none());
}

#[test]
fn esc_keeps_the_connections_and_none_checked_is_refused() {
    let mut model = profiles();
    press(&mut model, KeyCode::Char('c'));
    press(&mut model, KeyCode::Down);
    press(&mut model, KeyCode::Char(' '));
    let effects = press(&mut model, KeyCode::Enter);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(
        model.mcp_profiles.checklist.is_some(),
        "it stays, saying why"
    );
    assert!(
        model.mcp_profiles.status.contains("at least one"),
        "{}",
        model.mcp_profiles.status
    );
    press(&mut model, KeyCode::Esc);
    assert!(model.mcp_profiles.checklist.is_none());
    assert_eq!(model.agents_view, AgentsView::Profiles);
}

#[test]
fn q_flips_read_sql() {
    let mut model = profiles();
    let effects = press(&mut model, KeyCode::Char('q'));
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::SaveMcpProfileAccess { name, connections: None, reads: Some(false) }
                if name == "assistant"
        )),
        "{effects:?}"
    );
}

/// The buttons go while the pane asks something: they were drawn under the question,
/// and did nothing.
#[test]
fn a_question_or_the_checklist_has_the_pane_without_the_buttons() {
    let mut model = profiles();
    press(&mut model, KeyCode::Char('c'));
    let frame = paint(&mut model);
    assert!(!frame.contains("[e Disable]"), "{frame}");
    press(&mut model, KeyCode::Esc);
    press(&mut model, KeyCode::Char('x'));
    let frame = paint(&mut model);
    assert!(model.mcp_profiles.confirm.is_some());
    assert!(!frame.contains("[e Disable]"), "{frame}");
}

/// The list read again under the checklist -- a profile renamed at a shell -- does not
/// move what is saved onto another profile.
#[test]
fn the_checklist_saves_to_the_profile_it_was_opened_on() {
    let mut model = profiles();
    press(&mut model, KeyCode::Char('c'));
    let effects = update(&mut model, Action::ScreenTick);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadMcpProfiles)),
        "read again under the checklist: {effects:?}"
    );
    update(
        &mut model,
        Action::McpProfilesLoaded {
            profiles: vec![McpProfileSummary {
                name: "other".into(),
                enabled: true,
                connections: vec!["pg-dev".into()],
                ..McpProfileSummary::default()
            }],
        },
    );
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::SaveMcpProfileAccess { name, .. } if name == "assistant"
        )),
        "{effects:?}"
    );
}
