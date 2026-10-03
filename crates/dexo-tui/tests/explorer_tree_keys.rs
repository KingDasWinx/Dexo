//! Home, End, PageUp, PageDown, Left, Right and Space walk the explorer tree.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::model::{Focus, Model};
use dexo_tui::update::update;

fn press(model: &mut Model, code: KeyCode) {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

#[test]
fn home_and_end_jump_to_the_ends_of_the_tree() {
    let mut model = Model::default();
    model.apply_size(120, 30);
    let table = |name: &str| {
        dexo_driver_api::CatalogObject::new(
            dexo_driver_api::ObjectId::new(format!("table:{name}")),
            dexo_driver_api::ObjectKind::Table,
            dexo_driver_api::QualifiedName::new(None::<String>, Some("public"), name),
            None,
        )
    };
    model.explorer.replace_roots(dexo_driver_api::CatalogList {
        objects: vec![table("a"), table("b"), table("c")],
        restrictions: vec![],
    });
    model
        .explorer
        .select(dexo_driver_api::ObjectId::new("table:a"));
    model.focus = Focus::Explorer;
    press(&mut model, KeyCode::End);
    let last = model.explorer.selected.clone();
    press(&mut model, KeyCode::Home);
    let first = model.explorer.selected.clone();
    assert_ne!(first, last, "End and Home went to the same row");
    assert_eq!(
        model.explorer.selected_index(),
        0,
        "Home is the top of the tree"
    );
    press(&mut model, KeyCode::End);
    assert_eq!(model.explorer.selected, last);
}

/// One click on the arrow opens the node; on the name it only selects.
#[test]
fn a_click_on_the_arrow_opens_the_node() {
    use dexo_tui::mouse::{HitMap, HitTarget};
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.focus = Focus::Explorer;
    let profile = dexo_app::ConnectionProfile::new(
        dexo_app::ConnectionId(uuid::Uuid::nil()),
        None,
        "prod",
        "postgres",
        "local",
        serde_json::json!({}),
        dexo_app::SecretRef::new("ref".into()),
    );
    model.connections.profiles = vec![dexo_tui::screens::connections::ConnectionRow {
        temporary: false,
        profile,
        sessions: 0,
    }];
    // Live and folded by hand: the arrow of a connection not dialled dials it instead
    // (`explorer_open_connection.rs`).
    let session = dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1));
    model
        .connections
        .upsert_session(dexo_tui::screens::connections::SessionRow {
            id: session,
            connection: "prod".into(),
            transaction: dexo_driver_api::TransactionState::Idle,
            generation: 1,
            environment: "local".into(),
            read_only: false,
            driver: "postgres".into(),
        });
    model.connection.name = "prod".into();
    model.active_session = Some(session);
    model.session_generation = 1;
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, "prod");
    model
        .explorer
        .collapse(&dexo_tui::screens::explorer::connection_id("prod"));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    let (x, y) = model.hits.center(HitTarget::ExplorerTwistie(0));
    assert_ne!((x, y), (0, 0), "no arrow to click");
    update(
        &mut model,
        Action::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(
        model
            .explorer
            .selected_node()
            .is_some_and(|node| node.expanded)
    );
}

/// `PRIMARY` is every MySQL table's: under the table's name it says whose.
#[test]
fn an_index_is_named_for_its_table() {
    use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName};
    let mut model = Model::default();
    model.explorer.replace_roots(CatalogList {
        objects: vec![CatalogObject::new(
            ObjectId::new("table:orders"),
            ObjectKind::Table,
            QualifiedName::new(Some("qa4"), None::<String>, "orders"),
            None,
        )],
        restrictions: vec![],
    });
    model.explorer.apply_children(
        &ObjectId::new("table:orders"),
        CatalogList {
            objects: vec![CatalogObject::new(
                ObjectId::new("index:orders:PRIMARY"),
                ObjectKind::Index,
                QualifiedName::new(Some("qa4"), None::<String>, "PRIMARY"),
                Some(ObjectId::new("table:orders")),
            )],
            restrictions: vec![],
        },
    );
    model.explorer.select(ObjectId::new("index:orders:PRIMARY"));
    assert_eq!(
        model.explorer.selected_node().map(|n| n.qualified.as_str()),
        Some("qa4.orders.PRIMARY")
    );
}

/// With nothing open to type in, the welcome hands the keys to the explorer.
#[test]
fn the_welcome_leaves_the_focus_on_the_explorer_when_no_document_is_open() {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.onboarding.open = true;
    model.documents = vec![dexo_tui::model::EditorDocument::placeholder()];
    model.active_document = 0;
    model.focus = Focus::Editor;
    press(&mut model, KeyCode::Enter);
    assert!(!model.onboarding.open);
    assert_eq!(model.focus, Focus::Explorer);
}

/// A starred table in a schema nobody opened is listed by Show Favorites Only.
#[test]
fn favorites_only_lists_a_star_in_a_closed_schema() {
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    let profile = dexo_app::ConnectionProfile::new(
        dexo_app::ConnectionId(uuid::Uuid::nil()),
        None,
        "prod",
        "postgres",
        "local",
        serde_json::json!({}),
        dexo_app::SecretRef::new("ref".into()),
    );
    let mut model = Model::default();
    model.connections.profiles = vec![dexo_tui::screens::connections::ConnectionRow {
        temporary: false,
        profile,
        sessions: 1,
    }];
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, "prod");
    let schema = CatalogObject::new(
        ObjectId::new("schema:public"),
        ObjectKind::Schema,
        QualifiedName::new(None::<String>, Some("public"), "public"),
        None,
    );
    let table = CatalogObject::new(
        ObjectId::new("table:orders"),
        ObjectKind::Table,
        QualifiedName::new(None::<String>, Some("public"), "orders"),
        Some(ObjectId::new("schema:public")),
    );
    model
        .explorer
        .apply_favorites(&["table:orders".to_string()]);
    assert!(!model.explorer.has_all_favorites());
    model.explorer.graft_catalog("prod", vec![schema, table]);
    assert!(model.explorer.has_all_favorites());
    model.explorer.favorites_only = true;
    assert_eq!(
        model.explorer.visible_ids(),
        vec![ObjectId::new("table:orders")]
    );
}

/// One rule for every row: a click selects, a double click opens.
#[test]
fn a_connection_row_connects_on_a_double_click_like_every_other_row() {
    use dexo_tui::mouse::{HitMap, HitTarget};
    let profile = dexo_app::ConnectionProfile::new(
        dexo_app::ConnectionId(uuid::Uuid::nil()),
        None,
        "prod",
        "postgres",
        "local",
        serde_json::json!({}),
        dexo_app::SecretRef::new("ref".into()),
    );
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.connections.profiles = vec![dexo_tui::screens::connections::ConnectionRow {
        temporary: false,
        profile,
        sessions: 0,
    }];
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, "prod");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    let (x, y) = model.hits.center(HitTarget::ExplorerNode(0));
    let click = |model: &mut Model| {
        update(
            model,
            Action::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            }),
        )
    };
    let first = click(&mut model);
    assert!(first.is_empty(), "one click dialled: {first:?}");
    let second = click(&mut model);
    assert!(!second.is_empty(), "a double click did nothing");
}

/// `mysql.users [restricted]` explained nothing: the row says what it is and that the
/// account has no access, and Inspect says why.
#[test]
fn a_restricted_row_says_what_it_is() {
    use dexo_driver_api::QualifiedName;
    use dexo_driver_api::{CatalogList, CatalogObject, CatalogRestriction, ObjectId, ObjectKind};
    let mut model = Model::default();
    model.explorer.replace_roots(CatalogList {
        objects: vec![CatalogObject::new(
            ObjectId::new("db"),
            ObjectKind::Catalog,
            QualifiedName::new(None::<String>, None::<String>, "qa4"),
            None,
        )],
        restrictions: vec![CatalogRestriction {
            parent: None,
            capability: "mysql.users".into(),
            reason: "SELECT command denied".into(),
        }],
    });
    let node = model
        .explorer
        .nodes()
        .iter()
        .find(|node| node.restriction.is_some())
        .expect("a restricted row");
    assert_eq!(node.label, "Users");
}

/// A menu that lists an action shows the key that does it.
#[test]
fn every_row_of_the_node_menus_shows_its_key() {
    use dexo_tui::palette::{NodeMenuKind, node_menu_entries};
    let model = Model::default();
    for kind in [
        NodeMenuKind::Connection,
        NodeMenuKind::Relation,
        NodeMenuKind::Object,
    ] {
        for entry in node_menu_entries(&model, kind) {
            assert!(entry.shortcut.is_some(), "{} has no key", entry.title);
        }
    }
}
