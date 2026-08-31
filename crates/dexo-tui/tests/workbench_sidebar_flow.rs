use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::{Action, Effect};
use dexo_tui::model::{EditorDocument, Focus, Model};
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
fn n_types_in_the_editor_and_only_opens_the_connection_form_from_the_sidebar() {
    let mut model = Model::default();
    model.focus = Focus::Editor;

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

    let document = model.active_document();
    assert_eq!(
        document.connection_id.as_deref(),
        Some(connection_id.to_string().as_str())
    );
    assert_ne!(document.id, "scratch");
    assert!(document.path.is_none());
}

#[test]
fn up_on_first_sidebar_connection_stays_in_connections() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    model.focus = Focus::Explorer;
    model.explorer.sidebar_focus = SidebarFocus::Connections;

    let _ = update(&mut model, Action::ExplorerUp);

    assert_eq!(model.explorer.sidebar_focus, SidebarFocus::Connections);
    assert_eq!(model.explorer.connection_cursor, 0);
}

#[test]
fn editing_sidebar_connection_uses_sidebar_cursor() {
    let mut alternate = saved_profile();
    alternate.name = "staging".into();
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![saved_profile(), alternate]);
    model.focus = Focus::Explorer;
    model.explorer.connection_cursor = 1;

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
fn reactivating_open_sidebar_session_returns_to_editor_and_catalog() {
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
    model.explorer.sidebar_focus = SidebarFocus::Connections;

    let effects = update(&mut model, Action::ExplorerExpand);

    assert!(effects.is_empty());
    assert_eq!(model.explorer.sidebar_focus, SidebarFocus::Catalog);
    assert_eq!(model.focus, Focus::Editor);
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
