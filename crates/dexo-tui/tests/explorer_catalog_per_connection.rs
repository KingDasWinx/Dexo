//! A catalog answer lands under the connection whose session read it. Objects of two
//! connections share ids (`public` is `pg:schema:public` on every server): reading one
//! connection's schema filled, or failed, the other's of the same name too.
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::{
    CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName, TransactionState,
};
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::runtime::{OperationId, SessionId};
use dexo_tui::screens::connections::SessionRow;
use dexo_tui::screens::explorer::{ExplorerNode, NodeState, connection_id};
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

fn object(id: &ObjectId, kind: ObjectKind, name: &str, parent: ObjectId) -> CatalogObject {
    CatalogObject::new(
        id.clone(),
        kind,
        QualifiedName::new(None::<String>, Some("public"), name),
        Some(parent),
    )
}

fn public() -> ObjectId {
    ObjectId::new("pg:schema:public")
}

/// `alpha` and `beta` live, `alpha` in use; each lists a `public` schema, not read yet.
fn both_with_public() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model
        .connections
        .load_profiles(vec![profile("alpha", 1), profile("beta", 2)]);
    for (name, n) in [("alpha", 1), ("beta", 2)] {
        model.connections.upsert_session(SessionRow {
            id: session(n),
            connection: name.into(),
            transaction: TransactionState::Idle,
            generation: 1,
            environment: "local".into(),
            read_only: false,
            driver: "postgres".into(),
        });
    }
    model.connection.name = "alpha".into();
    model.connection.ready = true;
    model.active_session = Some(session(1));
    model.session_generation = 1;
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, "alpha");
    for name in ["alpha", "beta"] {
        model.explorer.apply_children(
            &connection_id(name),
            CatalogList {
                objects: vec![object(
                    &public(),
                    ObjectKind::Schema,
                    "public",
                    connection_id(name),
                )],
                restrictions: Vec::new(),
            },
        );
    }
    model
}

fn public_of<'a>(model: &'a Model, connection: &str) -> &'a ExplorerNode {
    model
        .explorer
        .roots
        .iter()
        .find(|root| root.id == connection_id(connection))
        .and_then(|root| root.children.iter().find(|child| child.id == public()))
        .expect("public listed")
}

#[test]
fn a_schemas_tables_land_only_under_the_connection_that_read_them() {
    let mut model = both_with_public();

    update(
        &mut model,
        Action::CatalogLoaded {
            operation: OperationId::new(),
            session: session(1).0.to_string(),
            generation: 1,
            parent: Some(public()),
            list: CatalogList {
                objects: vec![object(
                    &ObjectId::new("pg:table:public.only_on_alpha"),
                    ObjectKind::Table,
                    "only_on_alpha",
                    public(),
                )],
                restrictions: Vec::new(),
            },
            replace_roots: false,
        },
    );

    assert!(!public_of(&model, "alpha").children.is_empty());
    let beta = public_of(&model, "beta");
    assert!(beta.children.is_empty(), "{beta:?}");
    assert!(!beta.expanded);
}

#[test]
fn a_failed_read_marks_only_the_connection_that_failed() {
    let mut model = both_with_public();

    update(
        &mut model,
        Action::CatalogFailed {
            operation: OperationId::new(),
            session: session(1).0.to_string(),
            generation: 1,
            parent: Some(public()),
            message: "permission denied".into(),
            retryable: true,
        },
    );

    assert!(matches!(
        public_of(&model, "alpha").state,
        NodeState::Error { .. }
    ));
    assert!(!matches!(
        public_of(&model, "beta").state,
        NodeState::Error { .. }
    ));
}
