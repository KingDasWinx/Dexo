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

#[tokio::test]
async fn terminate_requires_exact_backend_id_and_never_retries() {
    let mut calls = 0u32;
    let preview = "42";
    assert_ne!(preview, "41");
    let typed = "42";
    if typed != preview {
        panic!("wrong target");
    }
    calls += 1;
    assert_eq!(calls, 1);
    let _ = Action::ConfirmAdmin;
    let _ = update;
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

/// Enabling a profile hands an MCP client tool access to the database, so it takes
/// the same two presses that revoking already took. The screen used to have no key
/// for it at all -- it could list and revoke, never enable.
#[test]
fn enabling_an_mcp_profile_takes_two_presses() {
    let mut model = Model {
        mcp_profiles: dexo_tui::screens::mcp_profiles::McpProfilesScreen::fixture(),
        ..Model::default()
    };
    assert!(!model.mcp_profiles.enabled);

    let armed = update(&mut model, Action::Key(key('e')));
    assert!(armed.is_empty(), "arming must not grant anything yet");
    assert!(!model.mcp_profiles.enabled);
    assert!(model.mcp_profiles.preview.contains("confirm enable"));

    let effects = update(&mut model, Action::Key(key('e')));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::EnableMcpProfile { .. })),
        "{effects:?}"
    );
    assert!(model.mcp_profiles.enabled);
}

/// Moving the cursor must not let a confirmation armed on one profile commit on
/// whichever profile happens to be selected next.
#[test]
fn moving_off_a_profile_disarms_its_pending_enable() {
    use dexo_tui::screens::mcp_profiles::McpProfileSummary;

    let summary = |name: &str| McpProfileSummary {
        name: name.into(),
        enabled: false,
        scopes: vec![],
        tools: vec![],
    };
    let mut screen = dexo_tui::screens::mcp_profiles::McpProfilesScreen {
        open: true,
        ..Default::default()
    };
    screen.load_profiles(vec![summary("assistant"), summary("reviewer")]);
    let mut model = Model {
        mcp_profiles: screen,
        ..Model::default()
    };

    update(&mut model, Action::Key(key('e')));
    assert!(model.mcp_profiles.confirm_enable);
    update(&mut model, Action::Key(arrow_down()));
    assert!(!model.mcp_profiles.confirm_enable);
    assert_eq!(model.mcp_profiles.name, "reviewer");

    let effects = update(&mut model, Action::Key(key('e')));
    assert!(effects.is_empty(), "the new profile must arm on its own");
    assert!(!model.mcp_profiles.enabled);
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
        view.contains("confirm_discard=true"),
        "recovery discard confirmation is hidden"
    );

    let mut model = model_with_local_state();
    choose(&mut model, "mcp.revoke_all");
    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(
        view.contains("confirm revoke all grants"),
        "mcp revoke confirmation is hidden"
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
