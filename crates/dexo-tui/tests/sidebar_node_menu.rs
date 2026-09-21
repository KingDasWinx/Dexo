//! The sidebar context menu. Its rows come from the same registry the palette reads,
//! so there is no second list of actions to drift out of date the way `ExplorerAction`
//! did before it.
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName};
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::palette::{NodeMenuKind, node_menu_entries, node_menu_items};
use dexo_tui::runtime::SessionId;
use dexo_tui::update::{node_menu_kind, update};

fn profile() -> ConnectionProfile {
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

fn model_on_connection() -> Model {
    let mut model = Model::default();
    model.apply_size(100, 30);
    model.connections.load_profiles(vec![profile()]);
    let profiles = model.connections.profiles.clone();
    model.explorer.sync_connection_roots(&profiles, "prod");
    model
        .explorer
        .select(dexo_tui::screens::explorer::connection_id("prod"));
    model
}

#[test]
fn a_connection_node_offers_the_connection_commands() {
    let mut model = model_on_connection();
    assert_eq!(node_menu_kind(&model), Some(NodeMenuKind::Connection));
    update(&mut model, Action::OpenNodeMenu);
    assert!(model.node_menu.open);

    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    // The four that were reachable only from inside the connections overlay, and the
    // one whose action had no trigger at all.
    for title in [
        "Test Connection",
        "Duplicate Connection",
        "Move to Group",
        "Delete Connection",
    ] {
        assert!(view.contains(title), "menu is missing {title}:\n{view}");
    }
}

#[test]
fn a_table_node_offers_the_object_commands_instead() {
    let mut model = model_on_connection();
    model.explorer.replace_roots(CatalogList {
        objects: vec![CatalogObject::new(
            ObjectId::new("table:orders"),
            ObjectKind::Table,
            QualifiedName::new(None::<String>, Some("public"), "orders"),
            None,
        )],
        restrictions: vec![],
    });
    model.explorer.select(ObjectId::new("table:orders"));
    assert_eq!(node_menu_kind(&model), Some(NodeMenuKind::Relation));

    let ids: Vec<_> = node_menu_items(NodeMenuKind::Relation).to_vec();
    assert!(ids.contains(&"explorer.data"));
    assert!(
        !ids.contains(&"connection.delete"),
        "a table cannot be deleted as a connection"
    );
}

/// Every id the menu lists has to resolve in the registry. A typo would otherwise show
/// up as a silently shorter menu.
#[test]
fn every_menu_id_resolves_to_a_command() {
    let model = model_on_connection();
    for kind in [
        NodeMenuKind::Connection,
        NodeMenuKind::Relation,
        NodeMenuKind::Object,
    ] {
        assert_eq!(
            node_menu_entries(&model, kind).len(),
            node_menu_items(kind).len(),
            "{kind:?} lists an id the registry does not know"
        );
    }
}

/// The menu reuses the palette's requirements, so a row it cannot run says why rather
/// than running anyway.
#[test]
fn picking_a_disabled_row_explains_instead_of_running() {
    let mut model = model_on_connection();
    update(&mut model, Action::OpenNodeMenu);
    let entries = node_menu_entries(&model, NodeMenuKind::Connection);
    let index = entries
        .iter()
        .position(|entry| entry.id == "admin.sessions")
        .expect("Inspect Sessions is on the connection menu");
    assert!(
        entries[index].disabled_reason.is_some(),
        "no session is open, so this row should be disabled"
    );
    model.node_menu.selected = index;

    let effects = update(&mut model, Action::Key(enter()));
    assert!(effects.is_empty(), "a disabled row must not dispatch");
    assert!(!model.node_menu.open);
    assert!(!model.admin.open, "the disabled screen opened anyway");
}

#[test]
fn the_a_key_opens_the_menu_and_esc_closes_it() {
    let mut model = model_on_connection();
    model.focus = dexo_tui::model::Focus::Explorer;
    update(&mut model, Action::Key(char_key('a')));
    assert!(model.node_menu.open, "`a` did not open the menu");
    update(&mut model, Action::Key(esc()));
    assert!(!model.node_menu.open, "Esc did not close the menu");
}

#[test]
fn running_a_usable_row_dispatches_its_command() {
    let mut model = model_on_connection();
    model.active_session = Some(SessionId(uuid::Uuid::from_u128(1)));
    model.session_generation = 1;
    update(&mut model, Action::OpenNodeMenu);
    let entries = node_menu_entries(&model, NodeMenuKind::Connection);
    let index = entries
        .iter()
        .position(|entry| entry.id == "connection.edit")
        .unwrap();
    model.node_menu.selected = index;
    update(&mut model, Action::Key(enter()));
    assert!(!model.node_menu.open);
    assert!(
        model.connection_form.open,
        "Edit Connection did not open the form"
    );
}

/// "Move to Group" is the edit form opened on the field that holds the group, because
/// that field is the only text input for one.
#[test]
fn move_to_group_lands_on_the_group_field() {
    let mut model = model_on_connection();
    update(&mut model, Action::EditConnectionGroup);
    assert!(model.connection_form.open);
    let focused = &model.connection_form.fields[model.connection_form.focus];
    assert_eq!(focused.label, "group");
}

fn char_key(c: char) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(c),
        crossterm::event::KeyModifiers::NONE,
    )
}

fn enter() -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    )
}

fn esc() -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    )
}

/// Right-click was not handled at all: `handle_mouse` matched only the left button and
/// the scroll wheel. It opens the menu now, and never as the only way in -- `a` does
/// the same thing from the keyboard.
#[test]
fn right_click_on_a_node_opens_its_menu() {
    use dexo_tui::mouse::{HitMap, HitTarget};

    let mut model = model_on_connection();
    model.mouse = true;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;

    let (x, y) = model.hits.center(HitTarget::ExplorerNode(0));
    update(
        &mut model,
        Action::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
            column: x,
            row: y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }),
    );
    assert!(model.node_menu.open, "right-click did not open the menu");
    assert_eq!(node_menu_kind(&model), Some(NodeMenuKind::Connection));
}

/// The header sheds from the left as the pane narrows. `Connections  [n]ew [e]dit` was
/// already 25 cells against a sidebar that defaults to 22, so it was being cut mid-word
/// before `[a]ctions` was ever added to it.
#[test]
fn the_sidebar_header_sheds_to_fit() {
    use dexo_tui::widgets::object_tree::sidebar_header;

    for width in [8usize, 12, 16, 20, 26, 40] {
        let header = sidebar_header(width);
        assert!(
            header.chars().count() <= width,
            "{width} cells overflowed with {header:?}"
        );
    }
    assert_eq!(
        sidebar_header(40),
        "Connections  [n]ew [e]dit [a]ctions",
        "a wide sidebar shows everything"
    );
    // The gateway to every other command outlives the label and `[e]dit`, which the
    // menu itself carries.
    assert!(sidebar_header(20).contains("[a]ctions"));
    assert!(sidebar_header(26).contains("[a]ctions"));
}
