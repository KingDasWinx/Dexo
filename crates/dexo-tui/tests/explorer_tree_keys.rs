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
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, "prod");
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
