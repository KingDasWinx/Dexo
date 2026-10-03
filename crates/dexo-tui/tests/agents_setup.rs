//! Agents' Setup: an agent pointed at Dexo's MCP server from the TUI -- the profile made
//! or picked, and the agent's config written. It took six commands at a shell.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::mcp::clients::{ClientState, McpClient};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_tui::model::Screen;
use dexo_tui::screen::agents::AgentsView;
use dexo_tui::screens::mcp_setup::{ClientRow, SetupProfile};
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

/// On the Setup view, with pg-dev in use, a SQLite file beside it, and no profile yet.
fn setup() -> Model {
    let mut model = Model::default();
    model.connections.load_profiles(vec![
        profile("pg-dev", "postgres", 1),
        profile("shop", "sqlite", 2),
        profile("my-dev", "mysql", 3),
    ]);
    model.connection.name = "pg-dev".into();
    update(&mut model, Action::OpenMcpSetup);
    update(
        &mut model,
        Action::McpProfilesLoaded {
            profiles: Vec::new(),
        },
    );
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
    assert_eq!(model.agents_view, AgentsView::Setup);
    paint(&mut model);
    model
}

/// Down to [Set up], then Enter.
fn set_up(model: &mut Model) -> Vec<Effect> {
    for _ in 0..20 {
        if model.mcp_setup.footer == dexo_tui::widgets::form::FooterFocus::Submit {
            break;
        }
        press(model, KeyCode::Down);
    }
    press(model, KeyCode::Enter)
}

#[test]
fn a_new_profile_on_the_connection_in_use_is_the_default() {
    let model = setup();
    let form = &model.mcp_setup;
    assert_eq!(form.profile, None);
    assert_eq!(form.name.as_str(), "assistant");
    assert_eq!(
        form.connections,
        vec![("my-dev".to_string(), false), ("pg-dev".to_string(), true)],
        "MCP serves Postgres and MySQL; the SQLite file is not offered"
    );
    let screen = dexo_tui::render::render_to_string(&model, 140, 40);
    assert!(screen.contains("Claude Code"), "{screen}");
    assert!(screen.contains("not set up"), "{screen}");
}

/// The detail is fields and the form, no sentences: the command is a button to copy,
/// not a line of a hundred and twenty characters.
#[test]
fn the_detail_is_fields_with_buttons_and_no_prose() {
    let model = setup();
    let screen = dexo_tui::render::render_to_string(&model, 140, 40);
    for text in [
        "[s Set up]",
        "[y Copy command]",
        " Status ",
        " Writes ",
        " Skill ",
        "Profile",
        "installed",
    ] {
        assert!(screen.contains(text), "{text}: {screen}");
    }
    assert!(!screen.contains("By hand"), "{screen}");
    assert!(!screen.contains("Writes Dexo's server into"), "{screen}");
    // Codex is not on this machine: the list says so.
    assert!(screen.contains("unreadable"), "{screen}");
}

#[test]
fn an_agent_not_on_this_machine_says_not_found() {
    let mut model = setup();
    update(
        &mut model,
        Action::McpClientsLoaded {
            clients: vec![
                row(McpClient::ClaudeCode, ClientState::NotSetUp),
                row(McpClient::Cursor, ClientState::NoFile),
            ],
            command: "/usr/bin/dexo".into(),
            project: "/p".into(),
        },
    );
    let screen = dexo_tui::render::render_to_string(&model, 140, 40);
    assert!(screen.contains("not found"), "{screen}");
    assert!(screen.contains("found"), "{screen}");
    press(&mut model, KeyCode::Down);
    let screen = dexo_tui::render::render_to_string(&model, 140, 40);
    assert!(screen.contains("not found on this machine"), "{screen}");
    assert!(screen.contains("new file"), "{screen}");
}

/// `s` sets the picked agent up from the list, with what the form holds.
#[test]
fn s_sets_up_from_the_list() {
    let mut model = setup();
    let effects = press(&mut model, KeyCode::Char('s'));
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::SetUpMcpClient {
                client: McpClient::ClaudeCode,
                ..
            }
        )),
        "{effects:?}"
    );
}

#[test]
fn enter_then_set_up_makes_the_profile_and_writes_the_config() {
    let mut model = setup();
    press(&mut model, KeyCode::Enter);
    assert_eq!(
        dexo_tui::screen::section(&model),
        dexo_tui::screen::Section::Detail
    );

    let effects = set_up(&mut model);

    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::SetUpMcpClient {
                client: McpClient::ClaudeCode,
                profile: SetupProfile::New { name, connections, reads: true },
                skill: true,
            } if name == "assistant" && connections == &["pg-dev".to_string()]
        )),
        "{effects:?}"
    );
    assert!(model.mcp_setup.busy);

    update(
        &mut model,
        Action::McpClientSetUp {
            result: Ok(vec!["Wrote /p/.mcp.json.".into()]),
        },
    );
    let screen = dexo_tui::render::render_to_string(&model, 140, 40);
    assert!(screen.contains("Wrote /p/.mcp.json."), "{screen}");
}

#[test]
fn the_name_is_typed_and_digits_go_into_it() {
    let mut model = setup();
    press(&mut model, KeyCode::Enter);
    press(&mut model, KeyCode::Down);
    for _ in 0.."assistant".len() {
        press(&mut model, KeyCode::Backspace);
    }
    for ch in "bot4".chars() {
        press(&mut model, KeyCode::Char(ch));
    }

    assert_eq!(model.mcp_setup.name.as_str(), "bot4");
    assert_eq!(
        model.agents_view,
        AgentsView::Setup,
        "4 was typed, not a view"
    );
}

#[test]
fn without_a_connection_it_says_so_and_writes_nothing() {
    let mut model = setup();
    press(&mut model, KeyCode::Enter);
    // Past the profile, the name and my-dev, to pg-dev's row, and off.
    for _ in 0..3 {
        press(&mut model, KeyCode::Down);
    }
    press(&mut model, KeyCode::Char(' '));

    let effects = set_up(&mut model);

    assert!(effects.is_empty(), "{effects:?}");
    assert!(matches!(
        &model.mcp_setup.outcome,
        Some(Err(why)) if why.contains("at least one connection")
    ));
}

#[test]
fn a_file_it_cannot_read_is_left_alone() {
    let mut model = setup();
    press(&mut model, KeyCode::Down);
    press(&mut model, KeyCode::Enter);

    let effects = set_up(&mut model);

    assert!(effects.is_empty(), "{effects:?}");
    assert!(matches!(
        &model.mcp_setup.outcome,
        Some(Err(why)) if why.contains("left as it is")
    ));
}

/// An agent's own command is copied from the list.
#[test]
fn y_copies_the_agents_own_command() {
    let mut model = setup();

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

/// With no profile yet the screen opens on Setup, not on an empty list of approvals.
#[test]
fn the_first_visit_opens_on_setup() {
    let mut model = Model::default();
    update(&mut model, Action::GoToScreen(Screen::Agents));
    assert_eq!(model.agents_view, AgentsView::Approvals);

    update(
        &mut model,
        Action::McpProfilesLoaded {
            profiles: Vec::new(),
        },
    );

    assert_eq!(model.agents_view, AgentsView::Setup);
}

/// Back from [Cancel] and in again, the form starts at its first row: it kept the one
/// left, and two Downs landed on Cancel again.
#[test]
fn the_form_is_entered_from_its_top() {
    let mut model = setup();
    press(&mut model, KeyCode::Enter);
    for _ in 0..20 {
        if model.mcp_setup.footer == dexo_tui::widgets::form::FooterFocus::Cancel {
            break;
        }
        press(&mut model, KeyCode::Down);
    }
    press(&mut model, KeyCode::Enter);
    assert_eq!(
        dexo_tui::screen::section(&model),
        dexo_tui::screen::Section::List
    );

    press(&mut model, KeyCode::Enter);

    assert_eq!(model.mcp_setup.row, 0);
    assert_eq!(
        model.mcp_setup.focused(),
        Some(dexo_tui::screens::mcp_setup::Row::Profile)
    );
}

/// On a small terminal the form scrolls to what has the focus: under the list, its
/// buttons were cut off below the bottom.
#[test]
fn a_small_terminal_scrolls_the_form_to_its_buttons() {
    let mut model = setup();
    press(&mut model, KeyCode::Enter);
    for _ in 0..20 {
        if model.mcp_setup.footer == dexo_tui::widgets::form::FooterFocus::Submit {
            break;
        }
        press(&mut model, KeyCode::Down);
    }

    let screen = dexo_tui::render::render_to_string(&model, 80, 16);

    assert!(screen.contains("[Set up]"), "{screen}");
}
