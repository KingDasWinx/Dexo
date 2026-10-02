//! A statement that runs says so, a second run does not queue behind it unseen, and the
//! commands that do nothing say why.
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

fn last_message(model: &Model) -> String {
    model
        .messages
        .last()
        .map(|entry| entry.message.clone())
        .unwrap_or_default()
}

/// Idle and busy screens were byte-identical while `pg_sleep(20)` ran.
#[test]
fn the_status_bar_says_a_query_is_running_and_how_to_stop_it() {
    let mut model = connected();
    let idle = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(!idle.contains("running"), "{idle}");

    model.set_sql("select pg_sleep(20);");
    update(&mut model, Action::ExecuteStatement);

    let busy = dexo_tui::render::render_to_string(&model, 120, 30);
    let status = busy.lines().last().unwrap_or_default();
    assert!(status.contains("running"), "{status}");
    assert!(status.contains("Ctrl+F2"), "{status}");
}

/// The second Ctrl+Enter was queued without a word and ran when the first ended.
#[test]
fn a_second_run_is_refused_while_one_is_running() {
    let mut model = connected();
    model.set_sql("select pg_sleep(20);");
    let first = update(&mut model, Action::ExecuteStatement);
    assert!(first.iter().any(|e| matches!(e, Effect::StartScript(_))));

    let second = update(&mut model, Action::ExecuteStatement);

    assert!(
        !second.iter().any(|e| matches!(e, Effect::StartScript(_))),
        "{second:?}"
    );
    assert!(
        last_message(&model).contains("already running"),
        "{}",
        last_message(&model)
    );
}

#[test]
fn execute_selection_with_nothing_selected_says_so() {
    let mut model = connected();
    model.set_sql("select 1;");

    let effects = update(&mut model, Action::ExecuteSelection);

    assert!(!effects.iter().any(|e| matches!(e, Effect::StartScript(_))));
    assert!(
        last_message(&model).contains("Select some SQL"),
        "{}",
        last_message(&model)
    );
}

/// The key stayed silent where the palette warned.
#[test]
fn cancel_with_nothing_running_says_so() {
    let mut model = connected();

    let effects = update(&mut model, Action::CancelQuery);

    assert!(effects.is_empty());
    assert_eq!(last_message(&model), "No query is running.");
}

/// A run the user stopped is not a failure: it was a red `error query cancelled`.
#[test]
fn a_cancelled_query_is_reported_as_cancelled_not_as_an_error() {
    let mut model = connected();
    model.set_sql("select pg_sleep(20);");
    update(&mut model, Action::ExecuteStatement);
    let key = model.results.tabs[0].key.operation.clone();

    update(
        &mut model,
        Action::QueryFailed {
            key,
            index: 0,
            message: "query cancelled".into(),
            details: Vec::new(),
            position: None,
        },
    );

    let last = model.messages.last().expect("no message");
    assert_eq!(last.message, "Query cancelled.");
    assert_eq!(last.severity, dexo_tui::model::Severity::Info);
    assert!(model.active_query.is_none());
}
