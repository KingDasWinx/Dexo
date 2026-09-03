use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName};
use dexo_tui::action::{Action, Effect};
use dexo_tui::model::{EditorDocument, Focus, Model};
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::explorer::connection_id;
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

fn sync_sidebar(model: &mut Model) {
    model.explorer.sync_connection_roots(
        &model.connections.profiles,
        model.connection.name.as_str(),
    );
}

fn select_connection(model: &mut Model, name: &str) {
    sync_sidebar(model);
    model.explorer.select(connection_id(name));
    if let Some(index) = model
        .connections
        .profiles
        .iter()
        .position(|row| row.profile.name == name)
    {
        model.connections.selected_profile = index;
    }
}

fn paint(model: &mut Model) {
    sync_sidebar(model);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
}

fn click_target(model: &mut Model, target: HitTarget) {
    let (column, row) = model.hits.center(target);
    let _ = update(
        model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
}

fn key_d() -> Action {
    Action::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
}

fn shift_d() -> Action {
    Action::Key(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT))
}

fn connect_prod(model: &mut Model, session: SessionId) {
    model.connections.load_profiles(vec![saved_profile()]);
    let _ = update(
        model,
        Action::ConnectionChanged {
            name: "prod".into(),
            ready: true,
            environment: "local".into(),
            session: Some(session),
            generation: 1,
            token: 0,
            read_only: false,
            driver: "postgres".into(),
        },
    );
}

#[test]
fn clicking_sidebar_new_opens_the_connection_form() {
    let mut model = Model::default();
    paint(&mut model);

    click_target(
        &mut model,
        HitTarget::Button(dexo_tui::mouse::HitButton::New),
    );

    assert!(model.connection_form.open);
}

#[test]
fn clicking_sidebar_edit_opens_the_selected_connection() {
    let mut alternate = saved_profile();
    alternate.name = "staging".into();
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![saved_profile(), alternate]);
    model.focus = Focus::Explorer;
    select_connection(&mut model, "staging");
    paint(&mut model);

    click_target(
        &mut model,
        HitTarget::Button(dexo_tui::mouse::HitButton::Edit),
    );

    assert_eq!(
        model
            .connection_form
            .editing
            .as_ref()
            .map(|profile| profile.name.as_str()),
        Some("staging")
    );
}

#[test]
fn n_types_in_the_editor_and_only_opens_the_connection_form_from_the_sidebar() {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };

    let _ = update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::empty())),
    );

    assert!(!model.connection_form.open);
    assert_eq!(model.active_document().text(), "n");

    model.focus = Focus::Explorer;
    let _ = update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::empty())),
    );

    assert!(model.connection_form.open);
}

#[test]
fn enter_on_sidebar_connection_emits_connect() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    model.focus = Focus::Explorer;
    select_connection(&mut model, "prod");

    let effects = update(&mut model, Action::ExplorerExpand);

    assert!(matches!(
        effects.as_slice(),
        [Effect::ConnectProfile { profile, token: 1 }] if profile.name == "prod"
    ));
}

#[test]
fn right_clicking_sidebar_connection_does_not_connect() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    select_connection(&mut model, "prod");
    paint(&mut model);
    let (column, row) = model.hits.center(HitTarget::ExplorerNode(0));

    let effects = update(
        &mut model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );

    assert!(effects.is_empty());
    assert_eq!(
        model.explorer.selected.as_ref(),
        Some(&connection_id("prod"))
    );
}

#[test]
fn ready_connection_switches_sidebar_to_catalog_without_stealing_focus() {
    let mut model = Model {
        focus: Focus::Explorer,
        ..Model::default()
    };

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

    assert_eq!(
        model.explorer.sidebar_focus,
        dexo_tui::screens::explorer::SidebarFocus::Catalog
    );
    assert_eq!(model.focus, Focus::Explorer);
}

#[test]
fn connect_opens_console_sql_for_connection() {
    let connection_id = uuid::Uuid::nil();
    let console = std::path::PathBuf::from("/tmp/sql/42/console.sql");
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    model.connection.name = "prod".into();

    let _ = update(
        &mut model,
        Action::ConnectionSqlReady {
            connection_id: connection_id.to_string(),
            files: vec![console.clone()],
            console: console.clone(),
            content: "select 1;".into(),
        },
    );

    let doc = model.active_document();
    assert_eq!(doc.path.as_deref(), Some(console.as_path()));
    assert_eq!(
        doc.connection_id.as_deref(),
        Some(connection_id.to_string().as_str())
    );
    assert_eq!(doc.text(), "select 1;");
}

#[test]
fn stale_connection_sql_result_leaves_documents_unchanged() {
    let console = std::path::PathBuf::from("/tmp/sql/stale/console.sql");
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    model.connection.name = "prod".into();

    let _ = update(
        &mut model,
        Action::ConnectionSqlReady {
            connection_id: uuid::Uuid::from_u128(42).to_string(),
            files: vec![console.clone()],
            console,
            content: "select stale;".into(),
        },
    );

    assert_eq!(model.documents.len(), 1);
    assert_eq!(model.active_document().path, None);
}

#[test]
fn ready_console_binds_an_existing_unbound_document() {
    let connection_id = uuid::Uuid::nil().to_string();
    let console = std::path::PathBuf::from("/tmp/sql/42/console.sql");
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    model.connection.name = "prod".into();
    model.documents = vec![EditorDocument::new_unique(
        "console.sql",
        Some(console.clone()),
        None,
    )];

    let _ = update(
        &mut model,
        Action::ConnectionSqlReady {
            connection_id: connection_id.clone(),
            files: vec![console.clone()],
            console,
            content: "select 1;".into(),
        },
    );

    assert_eq!(model.documents.len(), 1);
    assert_eq!(
        model.active_document().connection_id.as_deref(),
        Some(connection_id.as_str())
    );
}

#[test]
fn ready_connection_ensures_its_console_sql() {
    let connection_id = uuid::Uuid::nil().to_string();
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);

    let effects = update(
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

    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::EnsureConnectionSql { connection_id: id } if id == &connection_id
    )));
}

#[test]
fn ctrl_n_creates_a_document_bound_to_the_active_connection() {
    let connection_id = uuid::Uuid::from_u128(42);
    let mut profile = saved_profile();
    profile.id = ConnectionId(connection_id);
    let mut model = Model::default();
    model.connections.load_profiles(vec![profile]);
    model.connection.name = "prod".into();
    model.focus = Focus::Editor;

    let _ = update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)),
    );
    let _ = update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );

    let document = model.active_document();
    assert_eq!(
        document.connection_id.as_deref(),
        Some(connection_id.to_string().as_str())
    );
    assert_ne!(document.id, "scratch");
    assert!(document.path.is_none());
}

#[test]
fn up_on_first_sidebar_connection_stays_on_first_connection() {
    let mut alternate = saved_profile();
    alternate.name = "staging".into();
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![saved_profile(), alternate]);
    model.focus = Focus::Explorer;
    select_connection(&mut model, "prod");

    let _ = update(&mut model, Action::ExplorerUp);

    assert_eq!(
        model.explorer.selected.as_ref(),
        Some(&connection_id("prod"))
    );
}

#[test]
fn editing_sidebar_connection_uses_explorer_selection() {
    let mut alternate = saved_profile();
    alternate.name = "staging".into();
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![saved_profile(), alternate]);
    model.focus = Focus::Explorer;
    select_connection(&mut model, "staging");

    let _ = update(&mut model, Action::EditSelectedConnection);

    assert_eq!(
        model
            .connection_form
            .editing
            .as_ref()
            .map(|profile| profile.name.as_str()),
        Some("staging")
    );
}

#[test]
fn shift_d_on_sidebar_connection_closes_its_session() {
    let session = SessionId(uuid::Uuid::nil());
    let mut model = Model::default();
    connect_prod(&mut model, session);
    model.focus = Focus::Explorer;
    select_connection(&mut model, "prod");

    let effects = update(&mut model, shift_d());

    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::CloseSession { session: id } if *id == session
        )),
        "{effects:?}"
    );
}

#[test]
fn shift_d_on_child_closes_its_owning_connection() {
    let prod_session = SessionId(uuid::Uuid::from_u128(1));
    let staging_session = SessionId(uuid::Uuid::from_u128(2));
    let mut staging = saved_profile();
    staging.name = "staging".into();
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile(), staging]);
    for (name, session) in [("prod", prod_session), ("staging", staging_session)] {
        let _ = update(
            &mut model,
            Action::ConnectionChanged {
                name: name.into(),
                ready: true,
                environment: "local".into(),
                session: Some(session),
                generation: 1,
                token: 0,
                read_only: false,
                driver: "postgres".into(),
            },
        );
    }
    let schema = ObjectId::new("schema:prod:public");
    model.explorer.apply_children(
        &connection_id("prod"),
        CatalogList {
            objects: vec![CatalogObject::new(
                schema.clone(),
                ObjectKind::Schema,
                QualifiedName::new(Some("prod"), Some("public"), "public"),
                Some(connection_id("prod")),
            )],
            restrictions: Vec::new(),
        },
    );
    model.explorer.select(schema);
    model.focus = Focus::Explorer;

    let effects = update(&mut model, shift_d());

    assert!(matches!(
        effects.as_slice(),
        [Effect::CloseSession { session }] if *session == prod_session
    ));
}

#[test]
fn shift_d_on_duplicate_child_id_closes_the_selected_connection() {
    let prod_session = SessionId(uuid::Uuid::from_u128(1));
    let staging_session = SessionId(uuid::Uuid::from_u128(2));
    let mut staging = saved_profile();
    staging.name = "staging".into();
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![saved_profile(), staging]);
    for (name, session) in [("prod", prod_session), ("staging", staging_session)] {
        let _ = update(
            &mut model,
            Action::ConnectionChanged {
                name: name.into(),
                ready: true,
                environment: "local".into(),
                session: Some(session),
                generation: 1,
                token: 0,
                read_only: false,
                driver: "postgres".into(),
            },
        );
    }
    let shared_id = ObjectId::new("pg:schema:2200");
    for name in ["prod", "staging"] {
        model.explorer.replace_connection_catalog(
            name,
            CatalogList {
                objects: vec![CatalogObject::new(
                    shared_id.clone(),
                    ObjectKind::Schema,
                    QualifiedName::new(Some(name), Some("public"), "public"),
                    Some(connection_id(name)),
                )],
                restrictions: Vec::new(),
            },
            false,
        );
    }
    model.explorer.select_in_connection("staging", shared_id);
    model.focus = Focus::Explorer;

    let effects = update(&mut model, shift_d());

    assert!(
        matches!(
            effects.as_slice(),
            [Effect::CloseSession { session }] if *session == staging_session
        ),
        "{effects:?}"
    );
}

#[test]
fn shift_d_on_offline_connection_never_closes_another_session() {
    let prod_session = SessionId(uuid::Uuid::from_u128(1));
    let mut staging = saved_profile();
    staging.name = "staging".into();
    let mut model = Model::default();
    connect_prod(&mut model, prod_session);
    model
        .connections
        .load_profiles(vec![saved_profile(), staging]);
    model.focus = Focus::Explorer;
    select_connection(&mut model, "staging");

    let effects = update(&mut model, shift_d());

    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(model.active_session, Some(prod_session));
    assert!(
        model
            .messages
            .iter()
            .any(|message| message.contains("staging") && message.contains("disconnected"))
    );
}

#[test]
fn lowercase_d_on_catalog_object_still_opens_ddl() {
    let session = SessionId(uuid::Uuid::from_u128(1));
    let mut model = Model::default();
    connect_prod(&mut model, session);
    let schema = ObjectId::new("schema:prod:public");
    model.explorer.apply_children(
        &connection_id("prod"),
        CatalogList {
            objects: vec![CatalogObject::new(
                schema.clone(),
                ObjectKind::Schema,
                QualifiedName::new(Some("prod"), Some("public"), "public"),
                Some(connection_id("prod")),
            )],
            restrictions: Vec::new(),
        },
    );
    model.explorer.select(schema.clone());
    model.focus = Focus::Explorer;

    let effects = update(&mut model, key_d());

    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadObjectInspector { id, session: loaded, .. }]
            if id == &schema && *loaded == session
    ));
}

#[test]
fn editing_from_connections_overlay_uses_overlay_selection() {
    let mut alternate = saved_profile();
    alternate.name = "staging".into();
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![saved_profile(), alternate]);
    model.connections.open = true;
    model.connections.selected_profile = 1;
    model.explorer.connection_cursor = 0;

    let _ = update(&mut model, Action::EditSelectedConnection);

    assert_eq!(
        model
            .connection_form
            .editing
            .as_ref()
            .map(|profile| profile.name.as_str()),
        Some("staging")
    );
}

#[test]
fn second_activate_expands_collapsed_connected_folder() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
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
    model
        .explorer
        .collapse(&connection_id("prod"));
    model.focus = Focus::Explorer;
    select_connection(&mut model, "prod");

    let effects = update(&mut model, Action::ExplorerExpand);

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadCatalogChildren { .. }))
            || model
                .explorer
                .selected_node()
                .is_some_and(|node| node.expanded),
        "{effects:?}"
    );
}

#[test]
fn clicking_connected_connection_expands_collapsed_folder() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
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
    model
        .explorer
        .collapse(&connection_id("prod"));
    paint(&mut model);

    click_target(&mut model, HitTarget::ExplorerNode(0));

    assert!(
        model
            .explorer
            .selected_node()
            .is_some_and(|node| node.expanded),
        "expected connection folder to expand after click"
    );
}

#[test]
fn reactivating_a_different_open_session_keeps_sidebar_focus() {
    let mut alternate = saved_profile();
    alternate.name = "staging".into();
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![saved_profile(), alternate]);
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
    model.connections.upsert_session(dexo_tui::screens::connections::SessionRow {
        id: SessionId(uuid::Uuid::from_u128(2)),
        connection: "staging".into(),
        transaction: dexo_driver_api::TransactionState::Idle,
        generation: 2,
        environment: "local".into(),
        read_only: false,
        driver: "postgres".into(),
    });
    sync_sidebar(&mut model);
    model.focus = Focus::Explorer;
    select_connection(&mut model, "staging");

    let effects = update(&mut model, Action::ExplorerExpand);

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadCatalogChildren { .. }))
    );
    assert_eq!(model.connection.name, "staging");
    assert_eq!(model.focus, Focus::Explorer);
}

#[test]
fn activate_while_catalog_is_loading_does_not_collapse_folder() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
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
    model.focus = Focus::Explorer;
    select_connection(&mut model, "prod");

    let effects = update(&mut model, Action::ExplorerExpand);

    assert!(effects.is_empty());
    assert!(
        model
            .explorer
            .selected_node()
            .is_some_and(|node| node.expanded),
    );
}

#[test]
fn checkpoint_autosaves_dirty_document_with_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("console.sql");
    std::fs::write(&path, b"").unwrap();
    let mut model = Model::default();
    let mut doc = EditorDocument::new_unique("console.sql", Some(path), Some("c".into()));
    doc.sql.insert(0, "select 1").unwrap();
    model.documents = vec![doc];

    let effects = update(&mut model, Action::CheckpointTick);

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::AutosaveDocument { .. }))
    );
}

fn catalog_of(names: &[&str]) -> CatalogList {
    CatalogList {
        objects: names
            .iter()
            .map(|name| {
                CatalogObject::new(
                    ObjectId::new(format!("table:{name}")),
                    ObjectKind::Table,
                    QualifiedName::new(Some("db"), Some("public"), *name),
                    None,
                )
            })
            .collect(),
        restrictions: vec![],
    }
}

/// Every registered catalog hit must land on the screen row that actually draws
/// that node, otherwise a click selects the neighbouring object.
fn assert_catalog_hits_land_on_their_row(model: &Model, labels: &[&str]) {
    fn node_label(model: &Model, id: &ObjectId) -> Option<String> {
        fn walk(nodes: &[dexo_tui::screens::explorer::ExplorerNode], id: &ObjectId) -> Option<String> {
            for node in nodes {
                if node.id == *id {
                    return Some(node.label.clone());
                }
                if let Some(found) = walk(&node.children, id) {
                    return Some(found);
                }
            }
            None
        }
        walk(model.explorer.nodes(), id)
    }

    let (width, height) = (80u16, 24u16);
    let mut hits = HitMap::default();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    let screen: Vec<String> = dexo_tui::render::render_to_string(model, width, height)
        .lines()
        .map(str::to_string)
        .collect();
    let ids = model.explorer.visible_ids();

    for label in labels {
        let index = ids
            .iter()
            .position(|id| node_label(model, id).is_some_and(|name| name == *label))
            .unwrap_or_else(|| panic!("no visible node for {label}"));
        let (x, y) = hits.center(HitTarget::ExplorerNode(index));
        assert_eq!(
            hits.at(x, y),
            Some(HitTarget::ExplorerNode(index)),
            "node {label} has no hit target"
        );
        assert!(
            screen[y as usize].contains(label),
            "hit for node {label} points at row {y}: {:?}",
            screen[y as usize]
        );
    }
}

#[test]
fn catalog_hits_match_rendered_rows_under_active_connection() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    model.connection.name = "prod".into();
    model.explorer.sync_connection_roots(&model.connections.profiles, "prod");
    model.explorer.replace_connection_catalog(
        "prod",
        catalog_of(&["alpha", "beta"]),
        false,
    );

    assert_catalog_hits_land_on_their_row(&model, &["alpha", "beta"]);
}

#[test]
fn catalog_hits_match_rendered_rows_when_offline_with_cached_catalog() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    model.connection.name = "prod".into();
    model.explorer.sync_connection_roots(&model.connections.profiles, "prod");
    model
        .explorer
        .replace_connection_catalog("prod", catalog_of(&["alpha", "beta"]), true);

    assert_catalog_hits_land_on_their_row(&model, &["alpha", "beta"]);
}

#[test]
fn document_autosaved_marks_matching_revision_saved() {
    let mut model = Model::default();
    model
        .active_document_mut()
        .sql
        .insert(0, "select 1")
        .unwrap();
    let id = model.active_document().id.clone();
    let revision = model.active_document().sql.revision();

    let _ = update(&mut model, Action::DocumentAutosaved { id, revision });

    assert!(!model.active_document().is_dirty());
}
