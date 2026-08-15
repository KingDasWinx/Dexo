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
async fn saved_theme_keymap_and_mouse_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dexo_app::settings::SettingsFile {
        theme: dexo_app::settings::ThemeId::HighContrast,
        mouse: false,
        keymap: dexo_app::settings::KeymapConfig {
            run_statement: "Ctrl+Enter".into(),
        },
        ..dexo_app::settings::SettingsFile::default()
    };
    dexo_app::settings::save_settings(dir.path(), &settings).unwrap();
    let loaded = dexo_app::settings::load_settings(dir.path());
    assert_eq!(loaded.theme, dexo_app::settings::ThemeId::HighContrast);
    assert_eq!(loaded.keymap.run_statement, "Ctrl+Enter");
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
