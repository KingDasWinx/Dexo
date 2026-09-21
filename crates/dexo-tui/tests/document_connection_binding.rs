//! A document belongs to a connection: it executes there, the tab says so, and
//! switching tabs brings the session with it.
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::TransactionState;
use dexo_tui::Effect;
use dexo_tui::action::Action;
use dexo_tui::model::{EditorDocument, Model};
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::connections::SessionRow;
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

fn uuid_of(n: u128) -> String {
    uuid::Uuid::from_u128(n).to_string()
}

fn session_row(connection: &str, n: u128) -> SessionRow {
    SessionRow {
        id: SessionId(uuid::Uuid::from_u128(n + 100)),
        connection: connection.into(),
        transaction: TransactionState::Idle,
        generation: 1,
        environment: "local".into(),
        read_only: false,
        driver: "postgres".into(),
    }
}

/// Two connections, `alpha` live and active, `beta` saved but offline. Document 1 is
/// bound to alpha, document 2 to beta.
fn two_connections() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model
        .connections
        .load_profiles(vec![profile("alpha", 1), profile("beta", 2)]);
    model.connections.upsert_session(session_row("alpha", 1));
    model.connection.name = "alpha".into();
    model.connection.ready = true;
    model.active_session = Some(SessionId(uuid::Uuid::from_u128(101)));
    model.session_generation = 1;
    model.documents.push(EditorDocument::new_unique(
        "on-alpha.sql",
        None,
        Some(uuid_of(1)),
    ));
    model.documents.push(EditorDocument::new_unique(
        "on-beta.sql",
        None,
        Some(uuid_of(2)),
    ));
    model
}

#[test]
fn switching_to_a_tab_of_a_live_connection_activates_that_session() {
    let mut model = two_connections();
    model.connections.upsert_session(session_row("beta", 2));

    update(&mut model, Action::SelectDocument { index: 2 });
    assert_eq!(model.connection.name, "beta");
    assert_eq!(
        model.active_session,
        Some(SessionId(uuid::Uuid::from_u128(102)))
    );
}

#[test]
fn switching_to_a_tab_of_an_offline_connection_dials_it() {
    let mut model = two_connections();
    let effects = update(&mut model, Action::SelectDocument { index: 2 });
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::ConnectProfile { profile, .. } if profile.name == "beta"
        )),
        "no dial for the document's connection: {effects:?}"
    );
}

#[test]
fn switching_within_one_connection_costs_nothing() {
    let mut model = two_connections();
    model.documents[2].connection_id = Some(uuid_of(1));
    let effects = update(&mut model, Action::SelectDocument { index: 2 });
    assert!(
        effects.is_empty(),
        "the session was already the right one: {effects:?}"
    );
    assert_eq!(model.connection.name, "alpha");
}

#[test]
fn a_document_with_no_binding_leaves_the_connection_alone() {
    let mut model = two_connections();
    model.documents[2].connection_id = None;
    let effects = update(&mut model, Action::SelectDocument { index: 2 });
    assert!(effects.is_empty());
    assert_eq!(model.connection.name, "alpha");
}

/// Executing in a tab whose connection is offline must not run against whatever is
/// live. It dials, queues, and replays when the session lands.
#[test]
fn executing_on_an_offline_binding_queues_instead_of_running_elsewhere() {
    let mut model = two_connections();
    model.active_document = 2;
    model.documents[2].sql = dexo_sql::SqlDocument::new("select 1");

    let effects = update(&mut model, Action::ExecuteDocument);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ConnectProfile { .. })),
        "did not dial beta: {effects:?}"
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::StartScript(_))),
        "ran the query on alpha"
    );
    assert!(model.pending_execute.is_some(), "nothing was queued");
}

#[test]
fn a_stale_connect_does_not_fire_the_queued_execution() {
    let mut model = two_connections();
    model.active_document = 2;
    model.documents[2].sql = dexo_sql::SqlDocument::new("select 1");
    update(&mut model, Action::ExecuteDocument);
    let token = model.pending_execute.as_ref().unwrap().token;

    let effects = update(&mut model, connection_changed("beta", 2, token + 7));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::StartScript(_))),
        "a connect from somewhere else fired the query"
    );
}

#[test]
fn the_queued_execution_runs_once_its_connection_lands() {
    let mut model = two_connections();
    model.active_document = 2;
    model.documents[2].sql = dexo_sql::SqlDocument::new("select 1");
    update(&mut model, Action::ExecuteDocument);
    let token = model.pending_execute.as_ref().unwrap().token;

    let effects = update(&mut model, connection_changed("beta", 2, token));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::StartScript(_))),
        "the queued query never ran: {effects:?}"
    );
    assert!(model.pending_execute.is_none(), "the queue was not drained");
}

/// Connecting moves the active document to that connection's console; activating a
/// document moves the active connection to the document's. Both sides have to settle,
/// or switching tabs walks in a circle and the TUI stops drawing.
#[test]
fn the_connection_and_document_do_not_chase_each_other() {
    let mut model = two_connections();
    model.connections.upsert_session(session_row("beta", 2));

    for _ in 0..8 {
        let effects = update(&mut model, Action::SelectDocument { index: 2 });
        if effects.is_empty() {
            assert_eq!(model.connection.name, "beta");
            return;
        }
    }
    panic!("switching to the same tab never stopped producing effects");
}

fn connection_changed(name: &str, n: u128, token: u64) -> Action {
    Action::ConnectionChanged {
        name: name.into(),
        ready: true,
        environment: "local".into(),
        session: Some(SessionId(uuid::Uuid::from_u128(n + 100))),
        generation: 1,
        token,
        read_only: false,
        driver: "postgres".into(),
    }
}

/// Documents stored before the binding column existed come back with nothing in it, so
/// the whole strip would stay unlabelled until each one was touched. A console's path
/// is `sql/<connection uuid>/console.sql`, which says what the column does not.
#[test]
fn a_stored_console_is_bound_from_its_path() {
    let mut model = two_connections();
    let beta = uuid_of(2);
    let stored = dexo_storage::StoredDocument {
        id: "doc-console".into(),
        project_id: Some("p1".into()),
        title: "console.sql".into(),
        content: String::new(),
        path: Some(format!("/home/u/.local/share/dexo/sql/{beta}/console.sql")),
        fingerprint: None,
        kind: None,
        connection_id: None,
    };
    let restored = dexo_tui::update::document_from_stored_for_test(stored);
    assert_eq!(
        restored.connection_id.as_deref(),
        Some(beta.as_str()),
        "the console came back belonging to nobody"
    );
    model.documents.push(restored);
    let labels = dexo_tui::widgets::document_tabs::labels(&model);
    assert!(
        labels
            .iter()
            .any(|label| label.contains("beta\u{b7}console.sql")),
        "the tab still does not say whose console it is: {labels:?}"
    );
}

/// An unbound document runs on whatever is active, so using it is what settles the
/// question -- and from then on the tab answers it without being asked.
#[test]
fn using_an_unbound_document_binds_it_to_the_live_connection() {
    let mut model = two_connections();
    model
        .documents
        .push(EditorDocument::new_unique("query-4.sql", None, None));
    let index = model.documents.len() - 1;

    update(&mut model, Action::SelectDocument { index });
    assert_eq!(
        model.documents[index].connection_id.as_deref(),
        Some(uuid_of(1).as_str()),
        "the document stayed unbound after being used on alpha"
    );
    let labels = dexo_tui::widgets::document_tabs::labels(&model);
    assert!(
        labels
            .iter()
            .any(|label| label.contains("alpha\u{b7}query-4.sql")),
        "{labels:?}"
    );
}

/// With nothing connected there is nothing to bind to, and inventing one would be a
/// guess. It stays unbound and picks a connection the next time it is used.
#[test]
fn an_unbound_document_with_no_live_connection_stays_unbound() {
    let mut model = Model::default();
    model
        .documents
        .push(EditorDocument::new_unique("query-4.sql", None, None));
    let index = model.documents.len() - 1;
    update(&mut model, Action::SelectDocument { index });
    assert!(model.documents[index].connection_id.is_none());
}

/// Naming a console after its connection only happened when the console was created,
/// so a workspace restored from before that kept a row of `console.sql`. The rename
/// runs on restore too, once the profiles are loaded and there is a name to use.
#[test]
fn restored_consoles_take_their_connection_name() {
    let mut model = two_connections();
    for (n, name) in [(1u128, "alpha"), (2, "beta")] {
        let mut console = EditorDocument::new_unique("console.sql", None, Some(uuid_of(n)));
        console.title = "console.sql".into();
        model.documents.push(console);
        let _ = name;
    }
    dexo_tui::update::rename_restored_consoles_for_test(&mut model);

    let titles: Vec<_> = model
        .documents
        .iter()
        .map(|document| document.title.as_str())
        .collect();
    assert!(titles.contains(&"alpha"), "{titles:?}");
    assert!(titles.contains(&"beta"), "{titles:?}");
    assert!(
        !titles.contains(&"console.sql"),
        "a console kept the file name every connection shares: {titles:?}"
    );
}
