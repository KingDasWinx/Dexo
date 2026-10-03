//! History keeps every statement run, each with how it went: a script was kept as one
//! entry, and only when all of it ran -- a failure left no trace.
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::TransactionState;
use dexo_storage::HistoryOutcome;
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::connections::SessionRow;
use dexo_tui::{Effect, update};

fn connected() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.connections.load_profiles(vec![ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(1)),
        None,
        "pg-dev",
        "postgres",
        "local",
        serde_json::json!({"host":"h","port":5432,"username":"u","database":"qa0"}),
        SecretRef::new("r1".into()),
    )]);
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

fn kept(effects: &[Effect]) -> Vec<dexo_storage::NewHistoryEntry> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::PersistHistory(entry) => Some(entry.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn each_statement_is_kept_with_its_outcome_and_a_failure_with_its_error() {
    let mut model = connected();
    model.set_sql("select 1;\nselect * from nope;\nselect 3;");
    let effects = update(&mut model, Action::ExecuteDocument);
    let key = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::StartScript(request) => Some(request.key.clone()),
            _ => None,
        })
        .expect("the script runs");
    let mut effects = Vec::new();
    for action in [
        Action::QueryResultSetStarted {
            key: key.clone(),
            index: 0,
        },
        Action::QueryMeta {
            key: key.clone(),
            index: 0,
            columns: vec![dexo_driver_api::ColumnMeta {
                name: "?column?".into(),
                type_name: "int4".into(),
                nullable: true,
            }],
        },
        Action::QueryRows {
            key: key.clone(),
            index: 0,
            rows: vec![vec![dexo_driver_api::DbValue::I64(1)]],
        },
        Action::QueryResultSetFinished {
            key: key.clone(),
            index: 0,
            rows_affected: None,
            truncated: false,
        },
        Action::QueryResultSetStarted {
            key: key.clone(),
            index: 1,
        },
        Action::QueryFailed {
            key: key.clone(),
            index: 1,
            message: "relation \"nope\" does not exist".into(),
            details: Vec::new(),
            position: None,
            cancelled: false,
        },
    ] {
        effects.extend(update(&mut model, action));
    }
    let kept = kept(&effects);
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(kept[0].sql.trim_end_matches(';'), "select 1");
    assert_eq!(kept[0].outcome, HistoryOutcome::Ok);
    assert_eq!(kept[0].rows, Some(1));
    assert!(kept[0].duration_ms.is_some());
    assert_eq!(kept[0].connection_id.as_deref(), Some("pg-dev"));
    assert_eq!(kept[0].database.as_deref(), Some("qa0"));
    assert_eq!(kept[1].sql.trim_end_matches(';'), "select * from nope");
    assert_eq!(kept[1].outcome, HistoryOutcome::Failed);
    assert_eq!(
        kept[1].error.as_deref(),
        Some("relation \"nope\" does not exist")
    );
}

#[test]
fn a_statement_is_kept_once_and_a_cancelled_one_says_so() {
    let mut model = connected();
    model.set_sql("select pg_sleep(20);");
    let effects = update(&mut model, Action::ExecuteStatement);
    let key = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::StartScript(request) => Some(request.key.clone()),
            _ => None,
        })
        .expect("the statement runs");
    let mut effects = update(
        &mut model,
        Action::QueryFailed {
            key: key.clone(),
            index: 0,
            message: "canceling statement due to user request".into(),
            details: Vec::new(),
            position: None,
            cancelled: true,
        },
    );
    effects.extend(update(&mut model, Action::ScriptFinished { key }));
    let kept = kept(&effects);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].outcome, HistoryOutcome::Cancelled);
}
