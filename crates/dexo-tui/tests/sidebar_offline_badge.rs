//! `[offline]` marks the one connection whose tree is a saved snapshot. It went on every
//! connection in the sidebar as soon as any catalog was one, connected rows included, and
//! stayed on after a reconnect.
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::connections::SessionRow;
use dexo_tui::{Model, render::render_to_string};

fn profile(name: &str, n: u128) -> ConnectionProfile {
    ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(n)),
        None,
        name,
        "postgres",
        "local",
        serde_json::json!({"host":"localhost","port":5432,"username":"u","database":"d"}),
        SecretRef::new(format!("ref-{n}")),
    )
}

fn sidebar_rows(model: &Model) -> Vec<String> {
    render_to_string(model, 100, 30)
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn only_the_connection_showing_a_snapshot_is_marked_offline() {
    let mut model = Model::default();
    model.apply_size(100, 30);
    model.connections.load_profiles(vec![
        profile("alpha", 1),
        profile("beta", 2),
        profile("gamma", 3),
    ]);
    let profiles = model.connections.profiles.clone();
    // `alpha` is the active connection and has no session: its tree is the saved snapshot.
    model.explorer.sync_connection_roots(&profiles, "alpha");
    model.explorer.offline = true;
    model.connection.name = "alpha".into();
    // `beta` is connected.
    model.connections.upsert_session(SessionRow {
        id: SessionId(uuid::Uuid::from_u128(20)),
        connection: "beta".into(),
        transaction: dexo_driver_api::TransactionState::Idle,
        generation: 1,
        environment: "local".into(),
        read_only: false,
        driver: "postgres".into(),
    });
    let rows = sidebar_rows(&model);
    let row = |name: &str| {
        rows.iter()
            .find(|line| line.contains(name) && line.starts_with('\u{2502}'))
            .unwrap_or_else(|| panic!("no row for {name}:\n{}", rows.join("\n")))
            .clone()
    };
    assert!(row("alpha").contains("[offline]"), "{}", row("alpha"));
    assert!(!row("beta").contains("[offline]"), "{}", row("beta"));
    assert!(!row("gamma").contains("[offline]"), "{}", row("gamma"));
}

/// A connection in a group shows its folder in the sidebar, as Browse Connections does.
#[test]
fn a_grouped_connection_shows_its_group() {
    let mut model = Model::default();
    model.apply_size(100, 30);
    let mut grouped = profile("duck-new", 5);
    grouped.group_path = Some("grp-a".into());
    model.connections.load_profiles(vec![grouped]);
    let profiles = model.connections.profiles.clone();
    model.explorer.sync_connection_roots(&profiles, "");
    let rows = sidebar_rows(&model);
    assert!(
        rows.iter().any(|line| line.contains("grp-a/duck-new")),
        "{rows:?}"
    );
}

/// Deleting a connection says how many open documents lose it.
#[test]
fn the_delete_dialog_counts_the_documents_that_lose_the_connection() {
    let mut model = Model::default();
    model.apply_size(100, 30);
    let target = profile("sqlite-shop", 6);
    model.connections.load_profiles(vec![target.clone()]);
    let mut document = dexo_tui::model::EditorDocument::new_unique(
        "second.sql",
        None,
        Some(target.id.0.to_string()),
    );
    document.sql.insert(0, "select 1").unwrap();
    model.documents = vec![document];
    model.connections.ask_delete(Some(target));
    let screen = render_to_string(&model, 100, 30);
    assert!(
        screen.contains("1 open document loses it"),
        "the dialog is silent about the open document:\n{screen}"
    );
}

/// A container with a saved connection is said to be saved, not silently left out.
#[test]
fn a_container_that_is_already_saved_is_said_so() {
    let mut model = Model::default();
    model.apply_size(100, 30);
    let mut saved = profile("my-pg", 7);
    saved.config =
        serde_json::json!({"host": "127.0.0.1", "port": 5433, "username": "u", "database": "d"});
    model.connections.load_profiles(vec![saved]);
    let found = dexo_app::docker::from_inspect(
        r#"[{"Name": "/shop-pg", "Config": {"Image": "postgres:16",
             "Env": ["POSTGRES_USER=ana", "POSTGRES_PASSWORD=s3cret", "POSTGRES_DB=shop"]},
           "NetworkSettings": {"Ports": {"5432/tcp": [{"HostIp": "0.0.0.0", "HostPort": "5433"}]}}}]"#,
    );
    model.connections.open = true;
    dexo_tui::update(&mut model, dexo_tui::Action::DockerDiscovered(found));
    let screen = render_to_string(&model, 100, 30);
    assert!(
        screen.contains("already saved as connections: shop-pg"),
        "{screen}"
    );
}
