//! Agents' Profiles: an agent pointed at Dexo's MCP server from the TUI -- a profile made
//! with New profile, which connects its agent too, or an agent connected to one made.
//! It took six commands at a shell.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::mcp::clients::{ClientState, McpClient};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_tui::model::Screen;
use dexo_tui::screen::agents::AgentsView;
use dexo_tui::screens::mcp_clients::ClientRow;
use dexo_tui::screens::mcp_profiles::{
    AgentRow, ConnectionLine, Form, McpProfileSummary, ProfileRow,
};
use dexo_tui::{Action, Effect, Model, update};

fn profile(name: &str, driver: &str, n: u128) -> ConnectionProfile {
    ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(n)),
        None,
        name,
        driver,
        "local",
        serde_json::json!({"host":"h","port":5432,"username":"u","database":"d","path":"/tmp/x.db"}),
        SecretRef::new(format!("r{n}")),
    )
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    let effects = update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    paint(model);
    effects
}

fn paint(model: &mut Model) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
    let mut hits = dexo_tui::mouse::HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
}

fn row(client: McpClient, state: ClientState) -> ClientRow {
    ClientRow {
        client,
        path: format!("/home/u/{}.json", client.id()),
        skill: (client == McpClient::ClaudeCode).then(|| "/p/.claude/skills/dexo/SKILL.md".into()),
        state,
        found: client == McpClient::ClaudeCode,
    }
}

/// On Profiles, with pg-dev in use, a SQLite file beside it, and `profiles` made.
fn agents(profiles: Vec<McpProfileSummary>) -> Model {
    let mut model = Model::default();
    model.connections.load_profiles(vec![
        profile("pg-dev", "postgres", 1),
        profile("shop", "sqlite", 2),
        profile("my-dev", "mysql", 3),
    ]);
    model.connection.name = "pg-dev".into();
    update(&mut model, Action::OpenMcpProfiles);
    update(&mut model, Action::McpProfilesLoaded { profiles });
    update(
        &mut model,
        Action::McpClientsLoaded {
            clients: vec![
                row(McpClient::ClaudeCode, ClientState::NotSetUp),
                row(McpClient::Codex, ClientState::Unusable("not TOML".into())),
            ],
            command: "/usr/bin/dexo".into(),
            project: "/p".into(),
        },
    );
    assert_eq!(model.screen, Screen::Agents);
    assert_eq!(model.agents_view, AgentsView::Profiles);
    paint(&mut model);
    model
}

fn assistant() -> McpProfileSummary {
    McpProfileSummary {
        name: "assistant".into(),
        enabled: true,
        scopes: vec!["allow *".into()],
        connections: vec![ConnectionLine {
            name: "pg-dev".into(),
            ..ConnectionLine::default()
        }],
        ..McpProfileSummary::default()
    }
}

fn new_profile(model: &Model) -> &dexo_tui::screens::mcp_profiles::ProfileForm {
    match &model.mcp_profiles.form {
        Some(Form::Profile(form)) => form,
        other => panic!("no profile form: {other:?}"),
    }
}

/// Tab to the form's button, then Enter.
fn save(model: &mut Model) -> Vec<Effect> {
    for _ in 0..20 {
        let on = match &model.mcp_profiles.form {
            Some(Form::Profile(form)) => form.focus == ProfileRow::Save,
            Some(Form::Agent(form)) => form.focus == AgentRow::Connect,
            _ => true,
        };
        if on {
            break;
        }
        press(model, KeyCode::Tab);
    }
    press(model, KeyCode::Enter)
}

#[test]
fn a_new_profile_on_the_connection_in_use_is_the_default() {
    let mut model = agents(Vec::new());
    press(&mut model, KeyCode::Char('n'));
    let form = new_profile(&model);
    assert_eq!(form.name.as_str(), "assistant");
    let checked: Vec<(&str, bool)> = form
        .connections
        .iter()
        .map(|(choice, on)| (choice.name.as_str(), *on))
        .collect();
    assert!(checked.contains(&("pg-dev", true)) && checked.contains(&("my-dev", false)));
    assert!(
        !checked.iter().any(|(name, _)| *name == "shop"),
        "MCP serves no SQLite"
    );
    assert_eq!(
        form.agent_choice().map(|agent| agent.client),
        Some(McpClient::ClaudeCode),
        "the agent found here that no profile starts"
    );
}

/// Create makes the profile, seeing everything of pg-dev, and connects its agent.
#[test]
fn create_makes_the_profile_and_connects_its_agent() {
    let mut model = agents(Vec::new());
    press(&mut model, KeyCode::Char('n'));
    let effects = save(&mut model);
    let [Effect::SaveMcpProfile(save)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(save.name, "assistant");
    assert!(save.new && save.reads);
    assert_eq!(save.connections, ["pg-dev"]);
    assert_eq!(save.allow, ["*"]);
    assert_eq!(save.agent, Some((McpClient::ClaudeCode, true)));
    update(
        &mut model,
        Action::McpFormDone {
            result: Ok(vec!["✓ assistant made, enabled, on pg-dev".into()]),
        },
    );
    assert!(model.mcp_profiles.form.is_none());
    assert!(model.mcp_profiles.status.contains("assistant made"));
}

#[test]
fn the_name_is_typed_and_digits_go_into_it() {
    let mut model = agents(Vec::new());
    press(&mut model, KeyCode::Char('n'));
    for _ in 0.."assistant".len() {
        press(&mut model, KeyCode::Backspace);
    }
    for ch in "bot3".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    assert_eq!(new_profile(&model).name.as_str(), "bot3");
    assert_eq!(
        model.agents_view,
        AgentsView::Profiles,
        "3 was typed, not a view"
    );
}

#[test]
fn without_a_connection_it_says_so_and_saves_nothing() {
    let mut model = agents(Vec::new());
    press(&mut model, KeyCode::Char('n'));
    press(&mut model, KeyCode::Tab);
    let at = new_profile(&model)
        .connections
        .iter()
        .position(|(choice, _)| choice.name == "pg-dev")
        .unwrap();
    for _ in 0..at {
        press(&mut model, KeyCode::Right);
    }
    press(&mut model, KeyCode::Char(' '));
    let effects = save(&mut model);
    assert!(effects.is_empty(), "{effects:?}");
    let form = new_profile(&model);
    assert_eq!(form.focus, ProfileRow::Connections);
    assert!(
        form.error
            .as_ref()
            .is_some_and(|(_, why)| why.contains("at least one connection"))
    );
}

/// Connect agent on a file Dexo cannot read leaves it alone, and says so.
#[test]
fn a_file_it_cannot_read_is_left_alone() {
    let mut model = agents(vec![assistant()]);
    press(&mut model, KeyCode::Char('a'));
    let Some(Form::Agent(form)) = &model.mcp_profiles.form else {
        panic!("no agent form");
    };
    assert_eq!(
        form.choice().map(|agent| agent.client),
        Some(McpClient::ClaudeCode)
    );
    press(&mut model, KeyCode::Right);
    let effects = save(&mut model);
    assert!(effects.is_empty(), "{effects:?}");
    let Some(Form::Agent(form)) = &model.mcp_profiles.form else {
        panic!("stays open");
    };
    assert!(
        form.error
            .as_ref()
            .is_some_and(|why| why.contains("left as it is"))
    );
}

/// Connect agent writes the agent's config for the profile picked.
#[test]
fn connect_writes_the_agents_config_for_the_profile() {
    let mut model = agents(vec![assistant()]);
    press(&mut model, KeyCode::Char('a'));
    let effects = save(&mut model);
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::ConnectMcpAgent { client: McpClient::ClaudeCode, profile, skill: true }]
                if profile == "assistant"
        ),
        "{effects:?}"
    );
}

/// An agent's own command is copied from Connect agent.
#[test]
fn y_copies_the_agents_own_command() {
    let mut model = agents(vec![assistant()]);
    press(&mut model, KeyCode::Char('a'));
    let effects = press(&mut model, KeyCode::Char('y'));
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::CopyToClipboard { text }
                if text == "claude mcp add dexo -- /usr/bin/dexo mcp serve --profile assistant"
        )),
        "{effects:?}"
    );
}

/// With no profile yet the screen opens on Profiles, not on an empty list of approvals.
#[test]
fn the_first_visit_opens_on_profiles() {
    let mut model = Model::default();
    update(&mut model, Action::GoToScreen(Screen::Agents));
    assert_eq!(model.agents_view, AgentsView::Approvals);
    update(
        &mut model,
        Action::McpProfilesLoaded {
            profiles: Vec::new(),
        },
    );
    assert_eq!(model.agents_view, AgentsView::Profiles);
}
