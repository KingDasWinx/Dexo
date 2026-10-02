//! Where a failed statement is shown, and what the confirmation for one Dexo cannot read
//! is called.
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::TransactionState;
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::connections::SessionRow;
use dexo_tui::update;

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

fn fail(model: &mut Model, index: usize, position: Option<u32>) {
    let key = model.results.tabs[index].key.operation.clone();
    update(
        model,
        Action::QueryFailed {
            key,
            index,
            message: "no such table".into(),
            details: Vec::new(),
            position,
        },
    );
}

/// MySQL and SQLite name no position: the cursor still goes to the statement that failed.
#[test]
fn a_failure_without_a_position_moves_the_cursor_to_the_statement() {
    let mut model = connected();
    model.set_sql("select 1;\nselect * from nope;\nselect 3;");
    let end = model.active_document().text().chars().count();
    let _ = model.active_document_mut().sql.set_cursor(end);
    update(&mut model, Action::ExecuteDocument);

    fail(&mut model, 1, None);

    assert_eq!(model.active_document().cursor(), 10);
}

#[test]
fn the_underline_stops_before_the_semicolon() {
    let mut model = connected();
    model.set_sql("select * from nope;");
    update(&mut model, Action::ExecuteStatement);

    fail(&mut model, 0, Some(15));

    let (_, _, diagnostic) = model.editor.server_diagnostic.clone().expect("underlined");
    assert_eq!(diagnostic.byte_range, Some(14..18));
}

#[test]
fn a_sqlite_result_code_is_not_called_a_sqlstate() {
    let error = dexo_driver_api::DriverError::new(
        dexo_driver_api::DriverErrorCategory::Syntax,
        "no such table: nope",
    );
    let lines = dexo_tui::model::describe_query_error("select 1", &error, (0, 1));
    assert!(lines.iter().all(|line| !line.contains("SQLSTATE 1 ")));
}
