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
