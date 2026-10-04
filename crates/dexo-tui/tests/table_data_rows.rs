//! A table's rows in the grid: whose they are, what can be done to them, and how a
//! change is staged, reviewed and applied.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::data::{ColumnDef, TableMeta};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::{
    CatalogList, CatalogObject, ColumnMeta, DbValue, ObjectId, ObjectKind, TransactionState,
};
use dexo_tui::action::Action;
use dexo_tui::model::{EditorDocument, Focus, Model};
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::connections::SessionRow;
use dexo_tui::{Effect, update};

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

fn session(connection: &str, n: u128, read_only: bool) -> SessionRow {
    SessionRow {
        id: SessionId(uuid::Uuid::from_u128(n + 100)),
        connection: connection.into(),
        transaction: TransactionState::Idle,
        generation: 1,
        environment: "local".into(),
        read_only,
        driver: "postgres".into(),
    }
}

fn key(code: KeyCode) -> Action {
    Action::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// `alpha` is live and active; `beta` is live too, and its tree holds the table `orders`,
/// which is the node selected.
fn alpha_active_beta_tree() -> Model {
    let mut model = Model::default();
    model.apply_size(140, 40);
    model
        .connections
        .load_profiles(vec![profile("alpha", 1), profile("beta", 2)]);
    model.connections.upsert_session(session("alpha", 1, false));
    model.connections.upsert_session(session("beta", 2, false));
    model.connection.name = "alpha".into();
    model.connection.ready = true;
    model.active_session = Some(SessionId(uuid::Uuid::from_u128(101)));
    model.session_generation = 1;
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, "alpha");
    model.explorer.restore_connection_catalog(
        "beta",
        CatalogList {
            objects: vec![CatalogObject::new(
                ObjectId::new("table:orders"),
                ObjectKind::Table,
                dexo_app::parse_qualified("public.orders"),
                None,
            )],
            restrictions: vec![],
        },
    );
    model.explorer.select(ObjectId::new("table:orders"));
    model
}

/// With one connection active, `o` on a table in another connection's tree opened the
/// rows on the active one: the production and read-only guards were the wrong
/// connection's, and a Postgres table could be queried on a SQLite session.
#[test]
fn a_table_opened_from_a_connections_tree_belongs_to_that_connection() {
    let mut model = alpha_active_beta_tree();
    let effects = update(&mut model, Action::OpenObjectData);

    let table = model.active_document();
    assert!(table.kind.is_table());
    assert_eq!(
        table.connection_id.as_deref(),
        Some(uuid::Uuid::from_u128(2).to_string().as_str()),
        "the table is bound to the live connection, not the node's"
    );
    assert_eq!(model.connection.name, "beta");
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::LoadTableData { session, .. } if *session == SessionId(uuid::Uuid::from_u128(102))
    )));
    assert_eq!(
        model.explorer.selected,
        Some(ObjectId::new("table:orders")),
        "the table the user picked stays the selected node"
    );
}

/// The connection is only being dialled: the rows load when it lands, on it.
#[test]
fn a_table_of_an_offline_connection_dials_it_and_loads_when_it_lands() {
    let mut model = alpha_active_beta_tree();
    model
        .connections
        .remove_session(SessionId(uuid::Uuid::from_u128(102)));
    let effects = update(&mut model, Action::OpenObjectData);

    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::ConnectProfile { profile, .. } if profile.name == "beta"
    )));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadTableData { .. })),
        "the rows were asked of the live session, which is alpha's"
    );
    assert_eq!(
        model
            .pending_execute
            .as_ref()
            .map(|pending| &pending.action),
        Some(&Action::RefreshTableData)
    );
}

fn orders_meta() -> TableMeta {
    TableMeta {
        columns: vec![
            ColumnDef {
                name: "id".into(),
                primary_key: true,
                unique: true,
                nullable: false,
            },
            ColumnDef {
                name: "status".into(),
                primary_key: false,
                unique: false,
                nullable: true,
            },
        ],
    }
}

/// An open `orders` on `alpha`, loaded: columns, keys and three rows.
fn orders_open() -> Model {
    let mut model = Model::default();
    model.apply_size(140, 40);
    model.connections.load_profiles(vec![profile("alpha", 1)]);
    model.connections.upsert_session(session("alpha", 1, false));
    model.connection.name = "alpha".into();
    model.connection.ready = true;
    model.active_session = Some(SessionId(uuid::Uuid::from_u128(101)));
    model.session_generation = 1;
    let orders = dexo_app::parse_qualified("public.orders");
    model.documents.push(EditorDocument::new_table(
        orders.clone(),
        Some(uuid::Uuid::from_u128(1).to_string()),
    ));
    model.set_active_document(1);
    model.data.target = orders;
    model.data.table = orders_meta();
    model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
    model.results.set_columns(vec![
        ColumnMeta {
            name: "id".into(),
            type_name: "int8".into(),
            nullable: false,
        },
        ColumnMeta {
            name: "status".into(),
            type_name: "text".into(),
            nullable: true,
        },
    ]);
    model.results.append_rows(
        (1..=3)
            .map(|id| vec![DbValue::I64(id), DbValue::Text(format!("s{id}"))])
            .collect(),
    );
    model.results.select_cell(0, 1);
    model.focus = Focus::Results;
    model
}

/// F2 on a cell asks for the value, stages an UPDATE for the review, shows the new value
/// in the grid, and a second edit of the same row stays one statement.
#[test]
fn a_cell_is_edited_through_the_change_set_and_the_review() {
    let mut model = orders_open();
    update(&mut model, key(KeyCode::F(2)));
    assert!(model.data.cell_edit.is_some(), "F2 did not open the dialog");

    // Typing replaces the old value; Enter stages it.
    for ch in "shipped".chars() {
        update(&mut model, key(KeyCode::Char(ch)));
    }
    update(&mut model, key(KeyCode::Enter));
    assert!(model.data.cell_edit.is_none());
    assert_eq!(model.data.changes.pending().len(), 1);
    assert_eq!(
        model.results.rows()[0][1],
        DbValue::Text("shipped".into()),
        "the grid shows the new value where it will be"
    );
    assert!(
        dexo_tui::render::render_to_string(&model, 140, 40).contains("1 pending"),
        "nothing on screen says a change is waiting"
    );

    // Another cell of the same row, then the first one put back: nothing left.
    update(&mut model, key(KeyCode::F(2)));
    for ch in "s1".chars() {
        update(&mut model, key(KeyCode::Char(ch)));
    }
    update(&mut model, key(KeyCode::Enter));
    assert!(model.data.changes.pending().is_empty());

    // NULL has its own key.
    update(&mut model, key(KeyCode::F(2)));
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)),
    );
    assert_eq!(model.results.rows()[0][1], DbValue::Null);
    update(&mut model, Action::OpenReview);
    let review = model.data.review.as_ref().expect("a review");
    assert_eq!(
        review.preview_sql,
        "UPDATE public.orders SET status = NULL WHERE id = 1;"
    );
}

/// The column a row is found by is not edited from the grid, and say why.
#[test]
fn the_key_column_is_not_editable_and_says_why() {
    let mut model = orders_open();
    model.results.select_cell(0, 0);
    update(&mut model, key(KeyCode::F(2)));
    assert!(model.data.cell_edit.is_none());
    let message = model.messages.last().expect("a message").message.clone();
    assert!(message.contains("id is what finds this row"), "{message}");
}

/// Insert and Delete refuse before the form opens or the row is marked, each with its own
/// reason -- and one the connection gives comes first.
#[test]
fn changes_are_refused_up_front_with_the_reason() {
    let mut model = orders_open();
    model.connection.read_only = true;
    update(&mut model, Action::OpenInsertRow);
    assert!(!model.data.insert_form.open);
    assert!(
        model
            .messages
            .last()
            .unwrap()
            .message
            .contains("alpha is read-only"),
        "{:?}",
        model.messages.last()
    );
    update(&mut model, key(KeyCode::Delete));
    assert!(model.data.changes.pending().is_empty());

    let mut model = orders_open();
    model.data.table = TableMeta {
        columns: vec![ColumnDef {
            name: "note".into(),
            primary_key: false,
            unique: false,
            nullable: true,
        }],
    };
    model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
    update(&mut model, Action::OpenInsertRow);
    assert!(
        !model.data.insert_form.open,
        "the form opened on a keyless table"
    );
    assert!(
        model
            .messages
            .last()
            .unwrap()
            .message
            .contains("no primary key or unique column"),
        "{:?}",
        model.messages.last()
    );
}

/// A value the column cannot take keeps the form open and names the column.
#[test]
fn the_insert_form_shows_types_and_refuses_what_cannot_be_stored() {
    let mut model = orders_open();
    update(&mut model, Action::OpenInsertRow);
    let screen = dexo_tui::render::render_to_string(&model, 140, 40);
    assert!(screen.contains("id (int8) *"), "{screen}");
    assert!(screen.contains("status (text):"), "{screen}");

    for ch in "abc".chars() {
        update(&mut model, key(KeyCode::Char(ch)));
    }
    update(&mut model, key(KeyCode::Enter));
    assert!(model.data.insert_form.open, "a bad value closed the form");
    let error = model.data.insert_form.error.clone().unwrap();
    assert!(error.contains("id: `abc` is not a whole number"), "{error}");
    assert!(model.data.changes.pending().is_empty());
}

/// Ctrl+S with nothing staged said nothing, from a key; it opened an empty review.
#[test]
fn review_with_nothing_pending_says_so() {
    let mut model = orders_open();
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
    );
    assert!(model.data.review.is_none());
    assert_eq!(
        model.messages.last().unwrap().message,
        "No pending changes."
    );
}

/// The review has its buttons: Enter on Apply applies, Esc leaves the changes pending, and
/// once applied it closes.
#[test]
fn the_review_has_buttons_and_closes_when_applied() {
    let mut model = orders_open();
    update(&mut model, key(KeyCode::Delete));
    update(&mut model, Action::OpenReview);
    let screen = dexo_tui::render::render_to_string(&model, 140, 40);
    assert!(
        screen.contains("[Apply]") && screen.contains("[Cancel]"),
        "{screen}"
    );
    assert!(
        screen.contains("DELETE FROM public.orders WHERE id = 1;"),
        "{screen}"
    );

    update(&mut model, key(KeyCode::Esc));
    assert!(model.data.review.is_none());
    assert_eq!(
        model.data.changes.pending().len(),
        1,
        "Esc dropped the change"
    );

    update(&mut model, Action::OpenReview);
    // Right moves to Cancel and Enter takes it: the review closes, nothing was applied.
    update(&mut model, key(KeyCode::Right));
    update(&mut model, key(KeyCode::Enter));
    assert!(model.data.review.is_none());
    assert_eq!(model.data.changes.pending().len(), 1);

    update(&mut model, Action::OpenReview);
    let effects = update(&mut model, key(KeyCode::Enter));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ApplyMutations { .. })),
        "{effects:?}"
    );
    let generation = model.session_generation;
    update(
        &mut model,
        Action::MutationsApplied {
            generation,
            session: uuid::Uuid::from_u128(101).to_string(),
        },
    );
    assert!(
        model.data.review.is_none(),
        "the review stayed open after Apply"
    );
}

/// A table tab with rows staged is as unsaved as a file: closing it asks.
#[test]
fn closing_a_table_with_pending_changes_asks_first() {
    let mut model = orders_open();
    update(&mut model, key(KeyCode::Delete));
    assert!(
        dexo_tui::render::render_to_string(&model, 140, 40).contains("orders*"),
        "the tab does not say rows are waiting"
    );
    let documents = model.documents.len();
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
    );
    assert_eq!(model.documents.len(), documents, "the tab closed unasked");
    assert!(model.close_prompt.is_some());
}

/// Closing the tab on top brings up the connection of the one beneath, so the status bar
/// and the table actions answer for the tab on screen.
#[test]
fn closing_a_tab_brings_up_the_connection_of_the_tab_below() {
    let mut model = orders_open();
    model
        .connections
        .load_profiles(vec![profile("alpha", 1), profile("beta", 2)]);
    model.connections.upsert_session(session("alpha", 1, false));
    model.connections.upsert_session(session("beta", 2, false));
    let mut elsewhere = EditorDocument::new_unique(
        "elsewhere.sql",
        None,
        Some(uuid::Uuid::from_u128(2).to_string()),
    );
    elsewhere.kind = dexo_tui::model::DocumentKind::Console;
    model.documents.push(elsewhere);
    update(&mut model, Action::SelectDocument { index: 2 });
    assert_eq!(model.connection.name, "beta");

    update(&mut model, Action::CloseDocument);
    // What is on screen now is `orders` -- the document below -- on alpha.
    assert!(model.active_document().kind.is_table());
    assert_eq!(model.connection.name, "alpha");
}
