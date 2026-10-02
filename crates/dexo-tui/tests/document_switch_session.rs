//! Every way of changing the active document brings its session along: Alt+Left and
//! Alt+Right, closing one, and a new document made from the explorer.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::TransactionState;
use dexo_tui::Effect;
use dexo_tui::action::Action;
use dexo_tui::model::{EditorDocument, Focus, Model};
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

fn key(code: KeyCode, modifiers: KeyModifiers) -> Action {
    Action::Key(KeyEvent::new(code, modifiers))
}

/// `alpha` live and active, `beta` live too, and one document on each, `beta`'s last.
fn two_live_connections() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model
        .connections
        .load_profiles(vec![profile("alpha", 1), profile("beta", 2)]);
    model.connections.upsert_session(session_row("alpha", 1));
    model.connections.upsert_session(session_row("beta", 2));
    model.connection.name = "alpha".into();
    model.connection.ready = true;
    model.active_session = Some(SessionId(uuid::Uuid::from_u128(101)));
    model.session_generation = 1;
    model.documents = vec![
        EditorDocument::new_unique("on-alpha.sql", None, Some(uuid_of(1))),
        EditorDocument::new_unique("on-beta.sql", None, Some(uuid_of(2))),
    ];
    model.active_document = 0;
    model.focus = Focus::Editor;
    model
}

/// Alt+Right moved the tab and left the header and the status bar on the connection
/// before it, which only changed once something ran.
#[test]
fn alt_right_and_alt_left_bring_the_session_with_the_document() {
    let mut model = two_live_connections();

    update(&mut model, key(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(model.active_document, 1);
    assert_eq!(model.connection.name, "beta");
    assert_eq!(
        model.active_session,
        Some(SessionId(uuid::Uuid::from_u128(102)))
    );

    update(&mut model, key(KeyCode::Left, KeyModifiers::ALT));
    assert_eq!(model.active_document, 0);
    assert_eq!(model.connection.name, "alpha");
}

#[test]
fn alt_left_to_an_offline_connection_dials_it() {
    let mut model = two_live_connections();
    model.connections.remove_session(SessionId(uuid::Uuid::from_u128(102)));
    update(&mut model, Action::SelectDocument { index: 0 });

    let effects = update(&mut model, key(KeyCode::Right, KeyModifiers::ALT));

    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::ConnectProfile { profile, .. } if profile.name == "beta"
        )),
        "{effects:?}"
    );
}

/// Closing the active document put another one on screen under the header of the one
/// that was closed.
#[test]
fn closing_the_active_document_brings_the_next_ones_session() {
    let mut model = two_live_connections();
    update(&mut model, Action::SelectDocument { index: 1 });
    assert_eq!(model.connection.name, "beta");

    update(&mut model, Action::CloseDocument);

    assert_eq!(model.active_document().title, "on-alpha.sql");
    assert_eq!(model.connection.name, "alpha");
}

/// Ctrl+N from the explorer made the document on the connection last made active, and
/// the tab, the header and the first run all said so; the connection under the cursor
/// is the one the user is looking at.
#[test]
fn a_new_document_belongs_to_the_connection_under_the_explorer_cursor() {
    let mut model = two_live_connections();
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, "");
    model
        .explorer
        .select(dexo_tui::screens::explorer::connection_id("beta"));
    model.focus = Focus::Explorer;

    update(&mut model, Action::NewDocument);
    let dialog = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(dialog.contains("connection: beta"), "{dialog}");
    update(&mut model, key(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(model.documents.len(), 3);
    assert_eq!(model.active_document().connection_id, Some(uuid_of(2)));
    assert_eq!(model.connection.name, "beta");
}

#[test]
fn a_new_document_from_the_editor_belongs_to_the_active_connection() {
    let mut model = two_live_connections();

    update(&mut model, Action::NewDocument);
    update(&mut model, key(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(model.active_document().connection_id, Some(uuid_of(1)));
}

/// An emptied name used to close the dialog as if it had been answered.
#[test]
fn an_empty_document_name_is_refused_in_the_dialog() {
    let mut model = two_live_connections();
    update(&mut model, Action::NewDocument);
    update(&mut model, key(KeyCode::Backspace, KeyModifiers::NONE));

    update(&mut model, key(KeyCode::Enter, KeyModifiers::NONE));

    assert!(model.document_name_prompt.open, "the dialog closed");
    assert_eq!(model.documents.len(), 2);
    let frame = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(frame.contains("name cannot be empty"), "{frame}");
}

/// The dialog is as tall as what it holds, and a long name scrolls with the caret.
#[test]
fn the_name_dialog_has_no_blank_rows_and_scrolls_a_long_name() {
    let mut model = two_live_connections();
    update(&mut model, Action::NewDocument);
    for ch in "monthly_revenue_report_for_the_whole_company_2026_ZZ".chars() {
        update(&mut model, key(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    let frame = dexo_tui::render::render_to_string(&model, 100, 30);

    assert!(
        frame.contains("2026_ZZ"),
        "the end of the name is cut:\n{frame}"
    );
    let rows: Vec<&str> = frame
        .lines()
        .skip_while(|line| !line.contains("New document"))
        .take_while(|line| !line.contains('└'))
        .collect();
    assert!(rows.len() <= 5, "blank rows under the buttons:\n{frame}");
}
