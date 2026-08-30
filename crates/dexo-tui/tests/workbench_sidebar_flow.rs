use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_tui::action::{Action, Effect};
use dexo_tui::model::{Focus, Model};
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::explorer::SidebarFocus;
use dexo_tui::update;

fn saved_profile() -> ConnectionProfile {
    ConnectionProfile::new(
        ConnectionId(uuid::Uuid::nil()),
        None,
        "prod",
        "postgres",
        "local",
        serde_json::json!({"host":"localhost","port":5432,"username":"u","database":"d"}),
        SecretRef::new("ref-1".into()),
    )
}

#[test]
fn enter_on_sidebar_connection_emits_connect() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    model.focus = Focus::Explorer;
    model.explorer.sidebar_focus = SidebarFocus::Connections;

    let effects = update(&mut model, Action::ExplorerExpand);

    assert!(matches!(
        effects.as_slice(),
        [Effect::ConnectProfile { profile, token: 1 }] if profile.name == "prod"
    ));
}

#[test]
fn ready_connection_returns_focus_to_catalog_and_editor() {
    let mut model = Model::default();
    model.focus = Focus::Explorer;
    model.explorer.sidebar_focus = SidebarFocus::Connections;

    let _ = update(
        &mut model,
        Action::ConnectionChanged {
            name: "prod".into(),
            ready: true,
            environment: "local".into(),
            session: Some(SessionId(uuid::Uuid::nil())),
            generation: 1,
            token: 0,
            read_only: false,
            driver: "postgres".into(),
        },
    );

    assert_eq!(model.explorer.sidebar_focus, SidebarFocus::Catalog);
    assert_eq!(model.focus, Focus::Editor);
}
