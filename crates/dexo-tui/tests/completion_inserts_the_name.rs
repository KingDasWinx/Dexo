//! Accepting a table completion used to append an alias of its own invention --
//! `events_1m` came out as `events_1m e1`. It was noise in a single-table FROM, it was
//! inconsistent (`join t` aliased, `, t` did not), and after `INSERT INTO` it was a
//! syntax error: Postgres wants `AS` there and MySQL takes no alias at all.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers as M};
use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
use dexo_tui::action::Action;
use dexo_tui::model::{Focus, Model};
use dexo_tui::update::update;

fn complete(prefix: &str) -> String {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.focus = Focus::Editor;
    model.absorb_catalog(&[CatalogObject::new(
        ObjectId::new("table:events_1m"),
        ObjectKind::Table,
        QualifiedName::new(None::<String>, Some("public"), "events_1m"),
        None,
    )]);
    for ch in prefix.chars() {
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char(ch), M::NONE)),
        );
    }
    assert!(
        model.editor.completion_open,
        "no completion offered for {prefix:?}"
    );
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, M::NONE)),
    );
    model.active_document().text()
}

#[test]
fn accepting_a_table_inserts_the_table_and_nothing_else() {
    for (prefix, expected) in [
        ("select * from ev", "SELECT * FROM events_1m"),
        ("select * from a join ev", "SELECT * FROM a JOIN events_1m"),
        ("select * from a, ev", "SELECT * FROM a, events_1m"),
        ("select ev", "SELECT events_1m"),
        ("update ev", "UPDATE events_1m"),
        ("delete from ev", "DELETE FROM events_1m"),
        ("truncate ev", "TRUNCATE events_1m"),
        ("drop table ev", "DROP TABLE events_1m"),
        ("alter table ev", "ALTER TABLE events_1m"),
        ("create index on ev", "CREATE INDEX ON events_1m"),
    ] {
        assert_eq!(complete(prefix), expected, "after {prefix:?}");
    }
}

/// The one that was not a matter of taste.
#[test]
fn an_insert_target_is_never_aliased() {
    assert_eq!(complete("insert into ev"), "INSERT INTO events_1m");
}

fn typed(model: &mut Model, text: &str) {
    for ch in text.chars() {
        update(
            model,
            Action::Key(KeyEvent::new(KeyCode::Char(ch), M::NONE)),
        );
    }
}

fn two_schemas() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.focus = Focus::Editor;
    let table = |schema: &str, name: &str| {
        CatalogObject::new(
            ObjectId::new(format!("table:{schema}.{name}")),
            ObjectKind::Table,
            QualifiedName::new(None::<String>, Some(schema), name),
            None,
        )
    };
    model.absorb_catalog(&[table("reporting", "daily"), table("public", "customers")]);
    model
}

/// `daily` is in `reporting`: accepted bare, the query failed with "relation does not
/// exist".
#[test]
fn a_table_outside_the_default_schema_is_written_with_its_schema() {
    let mut model = two_schemas();
    typed(&mut model, "select * from dail");
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, M::NONE)),
    );
    assert_eq!(
        model.active_document().text(),
        "SELECT * FROM reporting.daily"
    );
    let mut model = two_schemas();
    typed(&mut model, "select * from cust");
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, M::NONE)),
    );
    assert_eq!(model.active_document().text(), "SELECT * FROM customers");
}

/// Enter after a table name already typed in full is the new line the user pressed it
/// for, not an acceptance of what is there that swallows it.
#[test]
fn enter_after_a_complete_name_starts_a_new_line() {
    let mut model = two_schemas();
    typed(&mut model, "select id from customers");
    assert!(model.editor.completion_open);
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, M::NONE)),
    );
    assert!(!model.editor.completion_open);
    assert_eq!(model.active_document().text(), "SELECT id FROM customers\n");
}
