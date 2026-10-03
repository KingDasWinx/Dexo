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
    assert!(!ran(&update(&mut model, Action::ExecuteDocument)));
    assert!(model.run_prompt.is_some());
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

/// A session that closes under the dialog can switch the editor to another connection
/// without a connection change; the answer still must not run there.
#[test]
fn a_session_closing_under_the_prompt_runs_nothing_on_the_next_connection() {
    let mut model = live("production", false, "delete from orders");
    let mut profiles: Vec<ConnectionProfile> = model
        .connections
        .profiles
        .iter()
        .map(|row| row.profile.clone())
        .collect();
    profiles.push(ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(2)),
        None,
        "other",
        "postgres",
        "production",
        serde_json::json!({"host":"h","port":5432,"username":"u","database":"d"}),
        SecretRef::new("r2".into()),
    ));
    model.connections.load_profiles(profiles);
    model.connections.upsert_session(SessionRow {
        id: SessionId(uuid::Uuid::from_u128(202)),
        connection: "other".into(),
        transaction: TransactionState::Idle,
        generation: 1,
        environment: "production".into(),
        read_only: false,
        driver: "postgres".into(),
    });
    update(&mut model, Action::ExecuteDocument);
    assert!(model.run_prompt.is_some());
    update(
        &mut model,
        Action::SessionClosed {
            session: SessionId(uuid::Uuid::from_u128(101)),
        },
    );
    type_text(&mut model, "shop");
    assert!(
        !ran(&press(&mut model, KeyCode::Enter)),
        "the answer for shop ran on {}",
        model.connection.name
    );
}

/// EXPLAIN ANALYZE runs the statement, so on a read-only connection a write is refused
/// before the dialog, not sent for the server to refuse.
#[test]
fn explain_analyze_of_a_write_is_refused_on_a_read_only_connection() {
    let mut model = live("local", true, "delete from orders");
    update(&mut model, Action::ConfirmExplainAnalyze);
    assert!(model.explain_prompt.is_none(), "the analyze dialog opened");
    let effects = update(&mut model, Action::RunExplainAnalyze);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::RunExplain { .. })),
        "EXPLAIN ANALYZE of a write was sent"
    );
    let mut model = live("local", true, "select * from orders");
    let effects = update(&mut model, Action::RunExplainAnalyze);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::RunExplain { analyze: true, .. })),
        "a read was refused"
    );
}

/// On MySQL a `#` line is a comment: it is neither a statement of its own that a
/// read-only connection refuses, nor a quote that merges what follows.
#[test]
fn a_mysql_hash_comment_is_a_comment() {
    let mut model = live("local", true, "# list them\nSHOW TABLES");
    model.connection.driver = "mysql".into();
    assert!(
        ran(&update(&mut model, Action::ExecuteDocument)),
        "{:?}",
        model.messages.last().map(|entry| entry.message.clone())
    );
    let mut model = live(
        "local",
        false,
        "# drop the customer's old table\nDROP TABLE customers_old;\nselect 1",
    );
    model.connection.driver = "mysql".into();
    update(&mut model, Action::ExecuteDocument);
    let prompt = model.run_prompt.as_ref().expect("DROP ran without asking");
    assert_eq!(prompt.statements, ["DROP TABLE customers_old", "select 1"]);
}

/// Types the connection's name into the production prompt and confirms it.
fn confirm_production(model: &mut Model, name: &str) -> Vec<Effect> {
    for ch in name.chars() {
        press(model, KeyCode::Char(ch));
    }
    press(model, KeyCode::Enter)
}

/// Every write that does not pass through the editor waits for the connection's name on
/// production, as a statement run from the editor does: DDL from the schema form, an
/// import, a restore, EXPLAIN ANALYZE of a write. They used to go ahead with an Enter.
#[test]
fn every_write_path_asks_for_the_name_on_production() {
    use dexo_tui::screens::transfer::TransferMode;
    // EXPLAIN ANALYZE of a write: the name is the only question asked.
    let mut model = live("production", false, "delete from orders");
    let analyzed = |effects: &[Effect]| {
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::RunExplain { analyze: true, .. }))
    };
    assert!(!analyzed(&update(
        &mut model,
        Action::ConfirmExplainAnalyze
    )));
    assert!(model.explain_prompt.is_none());
    assert!(model.production_prompt.is_some());
    assert!(!analyzed(&confirm_production(&mut model, "sho")));
    press(&mut model, KeyCode::Char('p'));
    assert!(analyzed(&press(&mut model, KeyCode::Enter)));
    // ... and of a read, as before: the analyze dialog only.
    let mut model = live("production", false, "select * from orders");
    update(&mut model, Action::ConfirmExplainAnalyze);
    assert!(model.explain_prompt.is_some() && model.production_prompt.is_none());

    // DDL from the schema form.
    let mut model = live("production", false, "");
    let change = model.schema_editor.to_change().ok();
    model.schema_editor.preview = Some(dexo_tui::screens::schema_editor::DdlPreviewState {
        target: "public.items".into(),
        sql: "CREATE TABLE public.items (id integer)".into(),
        risk: String::new(),
        warnings: Vec::new(),
        confirmation: dexo_app::schema::Confirmation::None,
        typed: Default::default(),
        confirmed: false,
        footer: dexo_tui::widgets::form::FooterFocus::Submit,
        error: None,
        scroll: 0,
        origin: dexo_tui::screens::schema_editor::PreviewOrigin::Form,
        change,
    });
    let applied = |effects: &[Effect]| {
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ApplyDdlChange { .. }))
    };
    assert!(!applied(&update(&mut model, Action::ApplyDdl)));
    assert!(model.production_prompt.is_some());
    assert!(applied(&confirm_production(&mut model, "shop")));

    // Import and restore.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("rows.csv");
    std::fs::write(&file, "id\n1\n").unwrap();
    for mode in [TransferMode::Import, TransferMode::Restore] {
        let mut model = live("production", false, "");
        model.data.target = dexo_app::parse_qualified("public.items");
        model.transfer.open = true;
        model.transfer.mode = mode;
        model.transfer.path.set_text(file.display().to_string());
        model.transfer.table.set_text("public.items");
        // The restore's own question was answered yes in the dialog.
        if mode == TransferMode::Restore {
            model.transfer.confirm = Some(dexo_tui::screens::transfer::TransferConfirm::Restore);
        }
        let started = |effects: &[Effect]| {
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::RunTransfer(_)))
        };
        assert!(
            !started(&update(&mut model, Action::SubmitTransfer)),
            "{mode:?}"
        );
        assert!(model.production_prompt.is_some(), "{mode:?}");
        assert!(started(&confirm_production(&mut model, "shop")), "{mode:?}");
    }
}
