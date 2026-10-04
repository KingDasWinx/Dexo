//! A transaction opened by SQL typed in the editor is as open as one from the palette:
//! the status bar shows it and quitting asks about it. And quitting asks about, and
//! stops, a statement still running.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::TransactionState;
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::connections::SessionRow;
use dexo_tui::{Effect, update};

fn connected() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    let profile = ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(1)),
        None,
        "pg-dev",
        "postgres",
        "local",
        serde_json::json!({"host":"h","port":5432,"username":"u","database":"d"}),
        SecretRef::new("r1".into()),
    );
    model.connections.load_profiles(vec![profile]);
    let session = SessionId(uuid::Uuid::from_u128(101));
    model.connections.upsert_session(SessionRow {
        id: session,
        connection: "pg-dev".into(),
        transaction: TransactionState::Idle,
        generation: 1,
        environment: "local".into(),
        read_only: false,
        driver: "postgres".into(),
    });
    model.connection.name = "pg-dev".into();
    model.connection.ready = true;
    model.active_session = Some(session);
    model.session_generation = 1;
    model
}

/// Runs `sql` and reports back as the runtime does when it succeeds.
fn run(model: &mut Model, sql: &str) {
    model.set_sql(sql);
    update(model, Action::ExecuteDocument);
    let key = model.results.tabs[0].key.operation.clone();
    update(model, Action::ScriptFinished { key });
}

#[test]
fn begin_in_the_editor_is_tracked_and_quitting_asks() {
    let mut model = connected();

    run(&mut model, "begin;");

    assert_eq!(model.transaction, TransactionState::Active);
    let frame = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(frame.contains("Transaction"), "{frame}");
    update(&mut model, Action::Quit);
    assert!(model.quit_prompt.is_some(), "quit did not ask");
    let frame = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(frame.contains("A transaction is open on pg-dev"), "{frame}");
}

#[test]
fn commit_and_rollback_close_it_again() {
    for closing in ["commit;", "rollback;", "end;", "ROLLBACK"] {
        let mut model = connected();
        run(&mut model, "begin;");
        assert_eq!(model.transaction, TransactionState::Active);

        run(&mut model, closing);

        assert_eq!(model.transaction, TransactionState::Idle, "{closing}");
        let effects = update(&mut model, Action::Quit);
        assert!(model.quit_prompt.is_none(), "{closing} still asks");
        assert!(effects.iter().any(|e| matches!(e, Effect::Shutdown)));
    }
}

/// `ROLLBACK TO` undoes to a savepoint; the transaction goes on.
#[test]
fn rollback_to_a_savepoint_leaves_the_transaction_open() {
    let mut model = connected();
    run(&mut model, "begin; savepoint a;");
    run(&mut model, "rollback to savepoint a;");
    assert_eq!(model.transaction, TransactionState::Active);
}

#[test]
fn a_statement_after_the_begin_that_fails_keeps_the_begin() {
    let mut model = connected();
    model.set_sql("begin; select nope;");
    update(&mut model, Action::ExecuteDocument);
    let key = model.results.tabs[0].key.operation.clone();
    update(
        &mut model,
        Action::QueryFailed {
            key,
            index: 1,
            message: "column \"nope\" does not exist".into(),
            details: Vec::new(),
            position: None,
            cancelled: false,
        },
    );
    assert_eq!(model.transaction, TransactionState::Active);
}

/// Ctrl+Q while a statement ran quit at once and left it running on the server.
#[test]
fn quitting_while_a_query_runs_asks_and_cancels_it() {
    let mut model = connected();
    model.set_sql("select pg_sleep(20);");
    update(&mut model, Action::ExecuteStatement);
    assert!(model.active_query.is_some());
    let operation = model.active_operation.expect("no operation");

    let effects = update(&mut model, Action::Quit);
    assert!(model.quit_prompt.is_some(), "quit did not ask");
    assert!(effects.is_empty());
    let frame = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(frame.contains("A query is still running"), "{frame}");

    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
    );
    let effects = update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CancelOperation(id) if *id == operation)),
        "{effects:?}"
    );
    assert!(effects.iter().any(|e| matches!(e, Effect::Shutdown)));
}
