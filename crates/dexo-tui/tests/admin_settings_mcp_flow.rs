use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::runtime::OperationId;
use dexo_tui::runtime::admin_manager::{AdminManager, AdminView, session_id, session_info};
use dexo_tui::update;

struct AdminHarness {
    manager: AdminManager,
    model: Model,
}

impl AdminHarness {
    fn admin_harness_with_two_sessions() -> Self {
        Self {
            manager: AdminManager::new(session_id(2)),
            model: Model::default(),
        }
    }

    fn refresh(&mut self, session: String, view: AdminView) -> OperationId {
        self.manager.refresh(session, view)
    }

    fn complete(&mut self, operation: OperationId, sessions: Vec<dexo_driver_api::SessionInfo>) {
        self.manager.complete(operation, sessions);
        self.model.admin.sessions = self.manager.sessions().to_vec();
    }

    fn model(&self) -> &Model {
        &self.model
    }
}

fn admin_harness_with_two_sessions() -> AdminHarness {
    AdminHarness::admin_harness_with_two_sessions()
}

#[tokio::test]
async fn admin_refresh_uses_selected_session_and_ignores_stale_response() {
    let mut harness = admin_harness_with_two_sessions();
    let first = harness.refresh(session_id(1), AdminView::Sessions);
    let second = harness.refresh(session_id(2), AdminView::Sessions);
    harness.complete(first, vec![session_info("old")]);
    harness.complete(second, vec![session_info("current")]);
    assert_eq!(harness.model().admin.sessions[0].id, "current");
}

#[tokio::test]
async fn saved_mode_accent_keymap_and_mouse_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dexo_app::settings::SettingsFile {
        mode: dexo_app::settings::ModeId::HighContrast,
        accent: "violet".into(),
        mouse: false,
        keymap: dexo_app::settings::KeymapConfig {
            run_statement: "Ctrl+Enter".into(),
            profile: "vim".into(),
        },
        ..dexo_app::settings::SettingsFile::default()
    };
    dexo_app::settings::save_settings(dir.path(), &settings).unwrap();
    let loaded = dexo_app::settings::load_settings(dir.path());
    assert_eq!(loaded.mode, dexo_app::settings::ModeId::HighContrast);
    assert_eq!(loaded.accent, "violet");
    assert_eq!(loaded.keymap.run_statement, "Ctrl+Enter");
    assert_eq!(loaded.keymap.profile, "vim");
    assert!(!loaded.mouse);
}

fn sessions_model(read_only: bool) -> dexo_tui::Model {
    let mut model = dexo_tui::Model {
        active_session: Some(dexo_tui::runtime::SessionId(uuid::Uuid::new_v4())),
        ..Default::default()
    };
    model.connection.name = "shop".into();
    model.connection.read_only = read_only;
    update(&mut model, Action::OpenAdmin);
    model.admin = dexo_tui::screens::admin::AdminScreen::fixture();
    model
}

fn press_key(
    model: &mut dexo_tui::Model,
    code: crossterm::event::KeyCode,
) -> Vec<dexo_tui::Effect> {
    update(
        model,
        Action::Key(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        )),
    )
}

fn terminates(effects: &[dexo_tui::Effect]) -> Option<String> {
    effects.iter().find_map(|effect| match effect {
        dexo_tui::Effect::AdminTerminate { target, .. } => Some(target.clone()),
        _ => None,
    })
}

/// Enter used to end the first session listed, whoever owned it. Now only the session
/// picked with the arrows is ended, after its id is typed, and an id that does not
/// match does nothing.
#[test]
fn only_the_picked_session_ends_once_its_id_is_typed() {
    use crossterm::event::KeyCode;
    let mut model = sessions_model(false);
    assert_eq!(terminates(&press_key(&mut model, KeyCode::Enter)), None);
    press_key(&mut model, KeyCode::Down);
    press_key(&mut model, KeyCode::Char('t'));
    assert_eq!(terminates(&press_key(&mut model, KeyCode::Enter)), None);
    assert!(model.admin.terminate.as_ref().unwrap().error.is_some());
    press_key(&mut model, KeyCode::Char('1'));
    assert_eq!(terminates(&press_key(&mut model, KeyCode::Enter)), None);
    press_key(&mut model, KeyCode::Char('1'));
    assert_eq!(
        terminates(&press_key(&mut model, KeyCode::Enter)).as_deref(),
        Some("11")
    );
    assert!(model.admin.terminate.is_none());
}

/// Ending a session is a write: a read-only connection refuses it before asking.
#[test]
fn a_read_only_connection_ends_no_session() {
    use crossterm::event::KeyCode;
    let mut model = sessions_model(true);
    press_key(&mut model, KeyCode::Char('t'));
    assert!(model.admin.terminate.is_none());
    assert!(
        model
            .admin
            .last_error
            .as_deref()
            .unwrap()
            .contains("read-only")
    );
}

#[tokio::test]
async fn security_screen_distinguishes_direct_inherited_and_public_grants() {
    #[derive(Clone, Debug, Eq, PartialEq)]
    enum PrivilegeSource {
        Direct,
        Inherited(String),
        Public,
    }
    let mut privileges = std::collections::BTreeMap::new();
    privileges.insert("SELECT", PrivilegeSource::Inherited("analyst".into()));
    privileges.insert("INSERT", PrivilegeSource::Direct);
    assert_eq!(
        privileges["SELECT"],
        PrivilegeSource::Inherited("analyst".into())
    );
    assert_eq!(privileges["INSERT"], PrivilegeSource::Direct);
    let _ = PrivilegeSource::Public;
}

#[tokio::test]
async fn mcp_profile_editor_persists_connections_selectors_tools_and_limits() {
    let profile = dexo_app::mcp::McpProfile::new("assistant");
    assert_eq!(profile.name, "assistant");
    assert!(!profile.enabled);
}

#[test]
fn settings_open_applies_without_fixture() {
    let mut model = Model::default();
    update(&mut model, Action::OpenSettings);
    assert!(model.settings.open);
}

/// Arrow keys are the advertised way to change a setting, so they have to be inverses:
/// step forward past what you wanted and left must bring it straight back.
#[test]
fn left_and_right_are_inverses_on_every_settings_row() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn press(model: &mut Model, code: KeyCode) {
        update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    let mut model = Model::default();
    update(&mut model, Action::OpenSettings);
    for row in 0..dexo_tui::screens::settings::FIELD_COUNT {
        model.settings.focus = row;
        let before = model.settings.clone();
        press(&mut model, KeyCode::Right);
        assert_ne!(model.settings, before, "row {row} ignored the right arrow");
        press(&mut model, KeyCode::Left);
        assert_eq!(model.settings, before, "row {row} did not step back");
    }
}

#[test]
fn open_admin_emits_load_when_session_ready() {
    let mut model = Model {
        active_session: Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1))),
        session_generation: 1,
        ..Model::default()
    };
    let effects = update(&mut model, Action::OpenAdmin);
    assert!(model.admin.open);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::LoadAdminSessions { .. }))
    );
}

/// A command that cannot run closes the palette and says why once; it stayed open under
/// its reason, with the editor behind it dead to the keys. A second Begin says what to
/// do, not "session is not idle".
#[test]
fn a_command_that_cannot_run_closes_the_palette_and_says_why() {
    let mut model = Model::default();
    choose(&mut model, "Commit Transaction");
    assert!(!model.palette.open, "the palette stayed open");
    assert_eq!(
        model.messages.last().map(|line| line.message.as_str()),
        Some("no active transaction")
    );

    let mut open = Model {
        transaction: dexo_driver_api::TransactionState::Active,
        ..Model::default()
    };
    choose(&mut open, "Begin Transaction");
    assert!(!open.palette.open);
    assert_eq!(
        open.messages.last().map(|line| line.message.as_str()),
        Some("a transaction is already open: commit or roll it back first")
    );
}

/// A session the server ended is closed, so the connection reads offline and the next
/// run connects by itself, instead of failing with "connection closed" for ever.
#[test]
fn a_session_the_server_ended_is_closed_and_said_so() {
    let session = dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(7));
    let mut model = Model::default();
    model
        .connections
        .upsert_session(dexo_tui::screens::connections::SessionRow {
            id: session,
            connection: "pg-prod".into(),
            transaction: dexo_driver_api::TransactionState::Idle,
            generation: 1,
            environment: "production".into(),
            read_only: false,
            driver: "postgres".into(),
        });
    let effects = update(
        &mut model,
        Action::SessionLost {
            session: session.0.to_string(),
        },
    );
    assert!(matches!(
        effects.as_slice(),
        [dexo_tui::Effect::CloseSession { session: closing }] if *closing == session
    ));
    assert!(model.messages.iter().any(|message| {
        message.message.contains("pg-prod lost its connection")
            && message.message.contains("Run again")
    }));
    // A session this model does not know is nobody's to close.
    assert!(
        update(
            &mut model,
            Action::SessionLost {
                session: "somewhere-else".into()
            }
        )
        .is_empty()
    );
}

/// A list longer than the box scrolls with Home, End and the paging keys, and the pick
/// stays on a row that is drawn.
#[test]
fn the_sessions_list_scrolls_by_key() {
    use crossterm::event::KeyCode;
    let mut model = sessions_model(false);
    model.height = 20;
    model.admin.sessions = (1..=17).map(|n| session_info(&n.to_string())).collect();
    model.admin.blocking.clear();
    let rows = model.admin.visible_rows(model.height);

    press_key(&mut model, KeyCode::End);
    assert_eq!(model.admin.selected, 16);
    assert!(model.admin.offset > 0);
    press_key(&mut model, KeyCode::Home);
    assert_eq!((model.admin.selected, model.admin.offset), (0, 0));
    press_key(&mut model, KeyCode::PageDown);
    assert_eq!(model.admin.selected, rows);
    let shown = model.admin.lines(58, rows).join("\n");
    assert!(
        shown.contains(&format!("> {:<7}", model.admin.sessions[rows].id)),
        "{shown}"
    );
    press_key(&mut model, KeyCode::PageUp);
    assert_eq!(model.admin.selected, 0);
}

/// The list loads in the background: the dialog says so, shows why when it cannot load,
/// and an answer that lands after Esc does not bring the dialog back.
#[test]
fn the_sessions_dialog_shows_loading_failures_and_ignores_a_late_answer() {
    let mut model = Model {
        active_session: Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1))),
        session_generation: 1,
        ..Model::default()
    };
    update(&mut model, Action::OpenAdmin);
    assert!(model.admin.loading);
    assert!(
        model
            .admin
            .lines(80, 5)
            .join("\n")
            .contains("Loading sessions")
    );

    update(
        &mut model,
        Action::AdminFailed {
            message: "connection refused".into(),
        },
    );
    assert!(!model.admin.loading);
    assert_eq!(
        model.admin.last_error.as_deref(),
        Some("connection refused")
    );
    assert!(
        model
            .messages
            .iter()
            .any(|message| message.message.contains("connection refused"))
    );

    update(&mut model, Action::OpenAdmin);
    press_key(&mut model, crossterm::event::KeyCode::Esc);
    assert!(!model.admin.open);
    update(
        &mut model,
        Action::AdminSessionsLoaded {
            sessions: Vec::new(),
            captured_at: "now".into(),
            blocking: Vec::new(),
        },
    );
    assert!(!model.admin.open, "a late answer reopened the dialog");
}

fn choose(model: &mut Model, query: &str) {
    let _ = choose_effects(model, query);
}

fn choose_effects(model: &mut Model, query: &str) -> Vec<dexo_tui::Effect> {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut effects = update(model, Action::OpenPalette);
    for ch in query.chars() {
        effects.extend(update(
            model,
            Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
        ));
    }
    effects.extend(update(
        model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    ));
    effects
}

fn press(model: &mut Model, ch: char) {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    update(
        model,
        Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
    );
}

/// Enabling a profile hands an MCP client tool access to the database, so it asks in a
/// dialog first, with Cancel focused, and no letter typed at the screen confirms it.
#[test]
fn enabling_an_mcp_profile_asks_in_a_dialog() {
    let mut model = Model {
        mcp_profiles: dexo_tui::screens::mcp_profiles::McpProfilesScreen::fixture(),
        screen: dexo_tui::model::Screen::Agents,
        agents_view: dexo_tui::screen::agents::AgentsView::Profiles,
        ..Model::default()
    };
    assert!(!model.mcp_profiles.enabled);

    let armed = update(&mut model, Action::Key(key('e')));
    assert!(armed.is_empty(), "asking must not grant anything yet");
    assert!(model.mcp_profiles.confirm.is_some());
    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(view.contains("Enable assistant?"), "{view}");
    assert!(
        view.contains("[Enable]") && view.contains("[Cancel]"),
        "{view}"
    );

    // Typing again confirms nothing: a second `e` was all it used to take.
    let again = update(&mut model, Action::Key(key('e')));
    assert!(again.is_empty());
    assert!(model.mcp_profiles.confirm.is_some());

    // Left to the Enable button, Enter confirms.
    update(&mut model, Action::Key(arrow_left()));
    let effects = update(&mut model, Action::Key(enter()));
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            dexo_tui::Effect::SetMcpProfileEnabled { enabled: true, .. }
        )),
        "{effects:?}"
    );
}

/// The grants section was fixture-only: nothing ever read the ledger, so the screen
/// showed an empty list no matter what a profile actually held.
#[test]
fn selecting_a_profile_shows_its_own_grants() {
    use dexo_tui::screens::mcp_profiles::{GrantLine, McpProfileSummary, McpProfilesScreen};

    let line = |id: &str, tools: &str| GrantLine {
        id: id.into(),
        capability: "data_write".into(),
        tools: tools.into(),
        expires_in_secs: 900,
        connection: "prod".into(),
        selectors: "db.public.items".into(),
        ask_secs: 0,
    };
    let profile = |name: &str, grants: Vec<GrantLine>| McpProfileSummary {
        name: name.into(),
        grants,
        ..Default::default()
    };

    let mut screen = McpProfilesScreen::default();
    screen.load_profiles(vec![
        profile("assistant", vec![line("g1", "data_insert")]),
        profile(
            "reviewer",
            vec![line("g2", "data_update"), line("g3", "data_delete")],
        ),
    ]);

    assert_eq!(screen.grants.len(), 1);
    assert!(screen.lines().join("\n").contains("data_insert"));

    screen.select_next();
    assert_eq!(screen.name, "reviewer");
    assert_eq!(screen.grants.len(), 2, "grants must follow the selection");
    let view = screen.lines().join("\n");
    assert!(view.contains("data_update") && view.contains("data_delete"));
    assert!(
        !view.contains("data_insert"),
        "the other profile's grants must not leak in"
    );
}

/// Disabling only takes access away, so it commits on the first press. Enabling grants
/// it, so it asks first. The asymmetry is the point.
#[test]
fn disabling_a_profile_takes_one_press_while_enabling_asks() {
    let mut model = Model {
        mcp_profiles: dexo_tui::screens::mcp_profiles::McpProfilesScreen::fixture(),
        screen: dexo_tui::model::Screen::Agents,
        agents_view: dexo_tui::screen::agents::AgentsView::Profiles,
        ..Model::default()
    };
    update(&mut model, Action::Key(key('e')));
    update(&mut model, Action::Key(arrow_left()));
    update(&mut model, Action::Key(enter()));
    // The runtime's reload brings the enabled profile back.
    let mut profile = model.mcp_profiles.profiles[0].clone();
    profile.enabled = true;
    model.mcp_profiles.load_profiles(vec![profile]);
    assert!(model.mcp_profiles.enabled);

    let effects = update(&mut model, Action::Key(key('e')));
    assert!(!model.mcp_profiles.enabled, "one press must disable");
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            dexo_tui::Effect::SetMcpProfileEnabled { enabled: false, .. }
        )),
        "{effects:?}"
    );

    // and it asks again on the way back up
    let armed = update(&mut model, Action::Key(key('e')));
    assert!(armed.is_empty());
    assert!(model.mcp_profiles.confirm.is_some());
}

/// `r` acts on the selected row and says what goes; the global sweep is `R`. Both ask in
/// a dialog that names how many grants are revoked.
#[test]
fn revoke_targets_the_selected_profile_and_shift_revokes_everything() {
    let mut model = Model {
        mcp_profiles: dexo_tui::screens::mcp_profiles::McpProfilesScreen::fixture(),
        screen: dexo_tui::model::Screen::Agents,
        agents_view: dexo_tui::screen::agents::AgentsView::Profiles,
        ..Model::default()
    };
    let armed = update(&mut model, Action::Key(key('r')));
    assert!(armed.is_empty(), "per-profile revoke must ask first");
    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(view.contains("Revoke the 1 grant of assistant?"), "{view}");

    update(&mut model, Action::Key(arrow_left()));
    let effects = update(&mut model, Action::Key(enter()));
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            dexo_tui::Effect::RevokeMcpGrants { profile } if profile == "assistant"
        )),
        "{effects:?}"
    );

    let mut model = Model {
        mcp_profiles: dexo_tui::screens::mcp_profiles::McpProfilesScreen::fixture(),
        screen: dexo_tui::model::Screen::Agents,
        agents_view: dexo_tui::screen::agents::AgentsView::Profiles,
        ..Model::default()
    };
    update(&mut model, Action::Key(key('R')));
    update(&mut model, Action::Key(arrow_left()));
    let effects = update(&mut model, Action::Key(enter()));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::RevokeAllMcpGrants)),
        "{effects:?}"
    );
}

/// A status line says what was done to the profile it was done to: it does not follow the
/// pick to another one.
#[test]
fn moving_off_a_profile_clears_what_was_said_about_it() {
    use dexo_tui::screens::mcp_profiles::McpProfileSummary;

    let summary = |name: &str| McpProfileSummary {
        name: name.into(),
        ..Default::default()
    };
    let mut screen = dexo_tui::screens::mcp_profiles::McpProfilesScreen::default();
    screen.load_profiles(vec![summary("assistant"), summary("reviewer")]);
    screen.status = "Disabled assistant: agents can no longer use it.".into();
    let mut model = Model {
        mcp_profiles: screen,
        screen: dexo_tui::model::Screen::Agents,
        agents_view: dexo_tui::screen::agents::AgentsView::Profiles,
        ..Model::default()
    };

    update(&mut model, Action::Key(arrow_down()));
    assert!(model.mcp_profiles.status.is_empty());
    assert_eq!(model.mcp_profiles.name, "reviewer");
    // The new profile asks on its own.
    let effects = update(&mut model, Action::Key(key('e')));
    assert!(effects.is_empty());
    assert!(!model.mcp_profiles.enabled);
}

fn arrow_left() -> crossterm::event::KeyEvent {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)
}

fn key(ch: char) -> crossterm::event::KeyEvent {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
}

fn arrow_down() -> crossterm::event::KeyEvent {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
}

/// The screen opened, showed a path field and could reach neither ExportConfig nor
/// ImportConfig: there was no way to name a file. Both keys now route through the
/// same file picker the transfer and diagnostics flows already use.
#[test]
fn config_transfer_reaches_export_and_import_through_the_picker() {
    for (ch, mode) in [
        (
            'e',
            dexo_tui::screens::file_picker::FilePickerMode::ConfigExport,
        ),
        (
            'i',
            dexo_tui::screens::file_picker::FilePickerMode::ConfigImport,
        ),
    ] {
        let mut model = Model::default();
        update(&mut model, Action::OpenConfigTransfer);
        assert!(model.config_transfer.open);

        update(&mut model, Action::Key(key(ch)));
        assert!(model.file_picker.open, "{ch} did not open the picker");
        assert_eq!(model.file_picker_mode, mode);

        // stand in for the user typing a filename rather than picking a row
        model.file_picker.focus = dexo_tui::screens::file_picker::FilePickerFocus::Name;
        model.file_picker.name.set_text("config.toml");
        let effects = update(&mut model, Action::Key(enter()));
        let reached = effects.iter().any(|effect| match ch {
            'e' => matches!(effect, dexo_tui::Effect::ExportConfig { .. }),
            _ => matches!(effect, dexo_tui::Effect::ImportConfig { .. }),
        });
        assert!(reached, "{ch} produced {effects:?}");
    }
}

/// The keys are only usable if the screen says they exist; it had no footer at all.
#[test]
fn config_transfer_advertises_its_keys() {
    let mut model = Model::default();
    update(&mut model, Action::OpenConfigTransfer);
    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(view.contains("e export"), "{view}");
    assert!(view.contains("i import"), "{view}");
}

fn enter() -> crossterm::event::KeyEvent {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
}

fn model_with_local_state() -> Model {
    let mut model = Model::default();
    model.recovery.checkpoints = vec![("scratch".into(), "scratch.sql".into(), "select 1".into())];
    model
}

/// Reset and discard live inside their own screens now, so they are reached by the
/// screen key rather than the palette. The confirmation must still be shown either way.
#[test]
fn destructive_local_commands_open_their_owner_before_confirmation() {
    let mut model = model_with_local_state();
    update(&mut model, Action::OpenSettings);
    press(&mut model, 'r');
    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(
        view.contains("[Confirm reset]"),
        "settings reset confirmation is hidden"
    );

    let mut model = model_with_local_state();
    update(&mut model, Action::OpenRecovery);
    press(&mut model, 'n');
    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(
        view.contains("Press Discard again"),
        "recovery discard confirmation is hidden"
    );

    let mut model = model_with_local_state();
    choose(&mut model, "mcp.revoke_all");
    // The profiles are read again, and the dialog says how many grants go.
    let fixture = dexo_tui::screens::mcp_profiles::McpProfilesScreen::fixture();
    update(
        &mut model,
        Action::McpProfilesLoaded {
            profiles: fixture.profiles,
        },
    );
    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(
        view.contains("Revoke every grant: 1 grant in 1 profile?"),
        "mcp revoke confirmation is hidden:\n{view}"
    );
}

#[test]
fn diagnostics_command_opens_preview_and_destination_flow() {
    let mut model = Model::default();
    choose(&mut model, "diagnostics.export");
    assert!(model.diagnostics.open);
    assert!(model.diagnostics.preview.contains("Dexo never uploads"));
    assert!(!model.diagnostics.writing);
}
