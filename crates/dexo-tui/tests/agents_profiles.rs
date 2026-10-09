//! A profile on Agents: what it reads and sees, what it may write on each connection and
//! which agents use it, on one page, changed with Edit, Writes and r; they took `dexo mcp
//! profile set`, `allow` and `grant` at a shell.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::mcp::WriteKind;
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_tui::screen::agents::AgentsView;
use dexo_tui::screens::mcp_profiles::{
    Access, ConnectionLine, Form, McpProfileSummary, ProfileRow,
};
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

fn assistant(name: &str) -> McpProfileSummary {
    McpProfileSummary {
        name: name.into(),
        enabled: true,
        raw_read: true,
        scopes: vec!["allow *".into()],
        connections: vec![ConnectionLine {
            name: "pg-dev".into(),
            environment: "local".into(),
            postgres: true,
            access: Some(Access {
                kinds: vec![WriteKind::Rows],
                expires_at: 1700,
                bypass: Vec::new(),
            }),
            ..ConnectionLine::default()
        }],
        kinds: WriteKind::ALL.to_vec(),
        ..McpProfileSummary::default()
    }
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
            profiles: vec![assistant("assistant")],
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

fn edit(model: &Model) -> &dexo_tui::screens::mcp_profiles::ProfileForm {
    match &model.mcp_profiles.form {
        Some(Form::Profile(form)) => form,
        other => panic!("no Edit: {other:?}"),
    }
}

#[test]
fn the_profile_is_one_page_of_fields_under_its_buttons() {
    let mut model = profiles();
    let frame = paint(&mut model);
    for text in [
        "[e Disable]",
        "[⏎ Edit]",
        "[x Delete]",
        "[n New]",
        "[R Stop all writes]",
        " Reads ",
        " Sees ",
        "CONNECTIONS",
        "✎ rows · 28 min left",
        "AGENTS",
        "● assistant",
    ] {
        assert!(frame.contains(text), "{text}: {frame}");
    }
}

#[test]
fn edit_checks_the_connections_and_save_saves_them() {
    let mut model = profiles();
    press(&mut model, KeyCode::Enter);
    let frame = paint(&mut model);
    assert!(frame.contains("[x] pg-dev"), "{frame}");
    assert!(frame.contains("[ ] my-dev"), "{frame}");
    assert!(
        !frame.contains("shop"),
        "MCP serves Postgres and MySQL: {frame}"
    );
    let at = edit(&model)
        .connections
        .iter()
        .position(|(choice, _)| choice.name == "my-dev")
        .unwrap();
    for _ in 0..at {
        press(&mut model, KeyCode::Right);
    }
    // Enter on the grid checks, as Space does; it saves only on Save.
    assert!(press(&mut model, KeyCode::Enter).is_empty());
    for _ in 0..10 {
        if edit(&model).focus == ProfileRow::Save {
            break;
        }
        press(&mut model, KeyCode::Tab);
    }
    assert!(paint(&mut model).contains("[ > Save ]"));
    let effects = press(&mut model, KeyCode::Enter);
    let [Effect::SaveMcpProfile(save)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(save.name, "assistant");
    assert!(!save.new);
    assert!(save.connections.contains(&"my-dev".to_string()));
    assert!(save.connections.contains(&"pg-dev".to_string()));
}

#[test]
fn esc_keeps_the_profile_and_none_checked_is_refused() {
    let mut model = profiles();
    press(&mut model, KeyCode::Enter);
    let at = edit(&model)
        .connections
        .iter()
        .position(|(choice, _)| choice.name == "pg-dev")
        .unwrap();
    for _ in 0..at {
        press(&mut model, KeyCode::Right);
    }
    press(&mut model, KeyCode::Char(' '));
    assert!(
        paint(&mut model).contains("! it may no longer write on pg-dev"),
        "unchecking takes the writes back, and the form says so"
    );
    for _ in 0..10 {
        if edit(&model).focus == ProfileRow::Save {
            break;
        }
        press(&mut model, KeyCode::Tab);
    }
    let effects = press(&mut model, KeyCode::Enter);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(edit(&model).focus, ProfileRow::Connections);
    assert!(paint(&mut model).contains("× check at least one connection"));
    press(&mut model, KeyCode::Esc);
    assert!(model.mcp_profiles.form.is_none());
    assert_eq!(model.agents_view, AgentsView::Profiles);
}

/// The buttons go while the pane asks something: they were drawn under the question,
/// and did nothing.
#[test]
fn a_question_or_a_form_has_the_pane_without_the_buttons() {
    let mut model = profiles();
    press(&mut model, KeyCode::Enter);
    let frame = paint(&mut model);
    assert!(!frame.contains("[e Disable]"), "{frame}");
    press(&mut model, KeyCode::Esc);
    press(&mut model, KeyCode::Char('x'));
    let frame = paint(&mut model);
    assert!(model.mcp_profiles.confirm.is_some());
    assert!(!frame.contains("[e Disable]"), "{frame}");
}

/// The list read again under a form -- a profile renamed at a shell -- does not move
/// what is saved onto another profile.
#[test]
fn a_form_saves_to_the_profile_it_was_opened_on() {
    let mut model = profiles();
    press(&mut model, KeyCode::Enter);
    let effects = update(&mut model, Action::ScreenTick);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadMcpProfiles)),
        "read again under the form: {effects:?}"
    );
    update(
        &mut model,
        Action::McpProfilesLoaded {
            profiles: vec![assistant("other")],
        },
    );
    for _ in 0..10 {
        if edit(&model).focus == ProfileRow::Save {
            break;
        }
        press(&mut model, KeyCode::Tab);
    }
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::SaveMcpProfile(save) if save.name == "assistant"
        )),
        "{effects:?}"
    );
}

/// Walked to a connection, r takes its writes back at once, and w opens Writes on it.
#[test]
fn r_and_w_act_on_the_walked_connection() {
    let mut model = profiles();
    let effects = press(&mut model, KeyCode::Char('r'));
    assert!(
        effects.is_empty(),
        "r needs a connection walked to: {effects:?}"
    );
    press(&mut model, KeyCode::Tab);
    let effects = press(&mut model, KeyCode::Char('r'));
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::RevokeMcpWrites { profile, connection }]
                if profile == "assistant" && connection == "pg-dev"
        ),
        "{effects:?}"
    );
    press(&mut model, KeyCode::Char('w'));
    let Some(Form::Writes(form)) = &model.mcp_profiles.form else {
        panic!("no Writes");
    };
    assert_eq!(form.connection_name(), "pg-dev");
    assert_eq!(
        form.checked,
        [WriteKind::Rows],
        "it opens on what it has there"
    );
}
