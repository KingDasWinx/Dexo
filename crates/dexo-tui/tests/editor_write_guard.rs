//! The SQL editor holds every run to the connection's policy: a read-only connection
//! refuses writes, production asks for its name, destructive statements ask first.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::TransactionState;
use dexo_tui::Effect;
use dexo_tui::action::Action;
use dexo_tui::model::{EditorDocument, Model};
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::connections::SessionRow;
use dexo_tui::update::update;

/// `shop` live and active, with one document bound to it holding `sql`.
fn live(environment: &str, read_only: bool, sql: &str) -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.connections.load_profiles(vec![ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(1)),
        None,
        "shop",
        "postgres",
        environment,
        serde_json::json!({"host":"h","port":5432,"username":"u","database":"d"}),
        SecretRef::new("r1".into()),
    )]);
    let session = SessionId(uuid::Uuid::from_u128(101));
    model.connections.upsert_session(SessionRow {
        id: session,
        connection: "shop".into(),
        transaction: TransactionState::Idle,
        generation: 1,
        environment: environment.into(),
        read_only,
        driver: "postgres".into(),
    });
    model.connection.name = "shop".into();
    model.connection.ready = true;
    model.connection.environment = environment.into();
    model.connection.read_only = read_only;
    model.connection.driver = "postgres".into();
    model.active_session = Some(session);
    model.session_generation = 1;
    model.documents.push(EditorDocument::new_unique(
        "shop.sql",
        None,
        Some(uuid::Uuid::from_u128(1).to_string()),
    ));
    model.active_document = model.documents.len() - 1;
    model.documents[model.active_document].sql = dexo_sql::SqlDocument::new(sql);
    model
}

fn ran(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, Effect::StartScript(_)))
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn type_text(model: &mut Model, text: &str) {
    for ch in text.chars() {
        press(model, KeyCode::Char(ch));
    }
}

#[test]
fn a_read_only_connection_refuses_a_write_and_sends_nothing() {
    let mut model = live("local", true, "delete from orders");
    assert!(!ran(&update(&mut model, Action::ExecuteDocument)));
    assert!(model.run_prompt.is_none());
    let message = model.messages.last().map(|entry| entry.message.clone());
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("read-only")),
        "{message:?}"
    );
}

#[test]
fn the_server_side_guard_cannot_be_switched_off_from_the_editor() {
    let mut model = live("local", true, "set default_transaction_read_only = off");
    assert!(!ran(&update(&mut model, Action::ExecuteDocument)));
}

#[test]
fn reads_run_without_asking_on_production() {
    let mut model = live("production", false, "select * from orders");
    assert!(ran(&update(&mut model, Action::ExecuteDocument)));
    assert!(model.run_prompt.is_none());
}

#[test]
fn production_runs_a_write_only_after_its_name_is_typed() {
    let mut model = live(
        "production",
        false,
        "update orders set paid = true where id = 7",
    );
    assert!(!ran(&update(&mut model, Action::ExecuteDocument)));
    assert!(model.run_prompt.is_some());
    type_text(&mut model, "Shop");
    assert!(
        !ran(&press(&mut model, KeyCode::Enter)),
        "a wrong-case name ran it"
    );
    assert!(model.run_prompt.is_some());
    for _ in 0..4 {
        press(&mut model, KeyCode::Backspace);
    }
    type_text(&mut model, "shop");
    assert!(ran(&press(&mut model, KeyCode::Enter)));
    assert!(model.run_prompt.is_none());
}

#[test]
fn an_unknown_label_like_prod_counts_as_production() {
    let mut model = live("prod", false, "insert into orders values (1)");
    update(&mut model, Action::ExecuteDocument);
    assert_eq!(
        model
            .run_prompt
            .as_ref()
            .and_then(|prompt| prompt.expected.clone())
            .as_deref(),
        Some("shop")
    );
}

#[test]
fn a_destructive_statement_asks_and_esc_runs_nothing() {
    let mut model = live("local", false, "delete from orders");
    assert!(!ran(&update(&mut model, Action::ExecuteDocument)));
    assert!(
        model
            .run_prompt
            .as_ref()
            .is_some_and(|prompt| prompt.expected.is_none())
    );
    assert!(!ran(&press(&mut model, KeyCode::Esc)));
    assert!(model.run_prompt.is_none());
}

#[test]
fn enter_on_the_default_focus_cancels_a_destructive_run() {
    let mut model = live("local", false, "drop table orders");
    update(&mut model, Action::ExecuteDocument);
    assert!(!ran(&press(&mut model, KeyCode::Enter)));
    assert!(model.run_prompt.is_none());
}

#[test]
fn run_on_a_destructive_prompt_sends_exactly_what_was_shown() {
    let mut model = live("local", false, "select 1;\ndelete from orders");
    update(&mut model, Action::ExecuteDocument);
    press(&mut model, KeyCode::Left);
    let effects = press(&mut model, KeyCode::Enter);
    let script = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::StartScript(request) => Some(request),
            _ => None,
        })
        .expect("Run sent nothing");
    assert_eq!(script.statements.len(), 2);
    assert!(script.statements[1].contains("delete from orders"));
}

#[test]
fn the_statement_under_the_cursor_is_guarded_too() {
    let mut model = live("local", true, "delete from orders");
    assert!(!ran(&update(&mut model, Action::ExecuteStatement)));
}

#[test]
fn a_connection_change_closes_the_run_prompt() {
    let mut model = live("production", false, "delete from orders");
    update(&mut model, Action::ExecuteDocument);
    assert!(model.run_prompt.is_some());
    update(
        &mut model,
        Action::ConnectionChanged {
            name: "other".into(),
            ready: true,
            environment: "local".into(),
            session: Some(SessionId(uuid::Uuid::from_u128(202))),
            generation: 1,
            token: 0,
            read_only: false,
            driver: "postgres".into(),
        },
    );
    assert!(model.run_prompt.is_none());
}
