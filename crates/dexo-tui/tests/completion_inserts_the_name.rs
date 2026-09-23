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
        ("select * from ev", "select * from events_1m"),
        ("select * from a join ev", "select * from a join events_1m"),
        ("select * from a, ev", "select * from a, events_1m"),
        ("select ev", "select events_1m"),
        ("update ev", "update events_1m"),
        ("delete from ev", "delete from events_1m"),
        ("truncate ev", "truncate events_1m"),
        ("drop table ev", "drop table events_1m"),
        ("alter table ev", "alter table events_1m"),
        ("create index on ev", "create index on events_1m"),
    ] {
        assert_eq!(complete(prefix), expected, "after {prefix:?}");
    }
}

/// The one that was not a matter of taste.
#[test]
fn an_insert_target_is_never_aliased() {
    assert_eq!(complete("insert into ev"), "insert into events_1m");
}
