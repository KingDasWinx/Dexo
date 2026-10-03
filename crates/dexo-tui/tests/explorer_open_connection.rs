//! Right (`explorer.open`) on a node whose connection is not the active one brings that
//! connection up first, as Enter on it does: it asked the active session for another
//! connection's objects, or no session at all, and the node said `[loading]` for ever.
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::{
    CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName, TransactionState,
};
use dexo_tui::Effect;
use dexo_tui::action::Action;
use dexo_tui::model::{Focus, Model};
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::connections::SessionRow;
use dexo_tui::screens::explorer::{NodeState, connection_id};
use dexo_tui::update::update;

fn profile(name: &str, n: u128) -> ConnectionProfile {
    ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(n)),
        None,
        name,
        "postgres",
        "local",
        serde_json::json!({"host":"h","port":5432,"username":"u","database":"d"}),
        SecretRef::new(format!("r{n}")),
    )
}

fn session(n: u128) -> SessionId {
    SessionId(uuid::Uuid::from_u128(n + 100))
}

fn session_row(connection: &str, n: u128) -> SessionRow {
    SessionRow {
        id: session(n),
        connection: connection.into(),
        transaction: TransactionState::Idle,
        generation: 1,
        environment: "local".into(),
        read_only: false,
        driver: "postgres".into(),
    }
}

/// `alpha` live and active, `beta` saved; the sidebar focused.
fn alpha_live() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model
        .connections
        .load_profiles(vec![profile("alpha", 1), profile("beta", 2)]);
    model.connections.upsert_session(session_row("alpha", 1));
    model.connection.name = "alpha".into();
    model.connection.ready = true;
    model.active_session = Some(session(1));
    model.session_generation = 1;
    model.focus = Focus::Explorer;
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, "alpha");
    model
}

#[test]
fn right_on_a_connection_not_dialled_connects_it() {
    let mut model = alpha_live();
    model.explorer.select(connection_id("beta"));

    let effects = update(&mut model, Action::ExplorerOpen);

    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::ConnectProfile { profile, .. } if profile.name == "beta"
        )),
        "{effects:?}"
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadCatalogChildren { .. })),
        "{effects:?}"
    );
}

#[test]
fn right_with_no_session_at_all_does_not_load_for_ever() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![profile("beta", 2)]);
    model.focus = Focus::Explorer;
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, "");
    model.explorer.select(connection_id("beta"));

    let effects = update(&mut model, Action::ExplorerOpen);

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ConnectProfile { .. })),
        "{effects:?}"
    );
    assert!(
        !matches!(
            model.explorer.selected_node().map(|node| &node.state),
            Some(NodeState::Loading(_))
        ),
        "{:?}",
        model.explorer.selected_node()
    );
}

#[test]
fn right_on_another_live_connections_node_reads_it_on_that_connection() {
    let mut model = alpha_live();
    model.connections.upsert_session(session_row("beta", 2));
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, "alpha");
    let schema = ObjectId::new("schema:beta:public");
    model.explorer.apply_children(
        &connection_id("beta"),
        CatalogList {
            objects: vec![CatalogObject::new(
                schema.clone(),
                ObjectKind::Schema,
                QualifiedName::new(Some("beta"), Some("public"), "public"),
                Some(connection_id("beta")),
            )],
            restrictions: Vec::new(),
        },
    );
    model.explorer.select(schema.clone());

    let effects = update(&mut model, Action::ExplorerOpen);

    assert_eq!(model.connection.name, "beta");
    assert_eq!(model.active_session, Some(session(2)));
    assert_eq!(model.explorer.selected.as_ref(), Some(&schema));
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadCatalogChildren { parent: Some(parent), session: asked, .. }
                if *parent == schema && *asked == session(2)
        )),
        "{effects:?}"
    );
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadCatalogChildren { session: asked, .. } if *asked == session(1)
        )),
        "{effects:?}"
    );
}
