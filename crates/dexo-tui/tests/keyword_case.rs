//! Reserved words go in capitals as they are typed, the way the formatter writes them.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::{Focus, Model, update};

fn typed(text: &str) -> String {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    for ch in text.chars() {
        let code = if ch == '\n' {
            KeyCode::Enter
        } else {
            KeyCode::Char(ch)
        };
        // Esc keeps the popup from taking an Enter meant as a newline.
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        update(
            &mut model,
            Action::Key(KeyEvent::new(code, KeyModifiers::NONE)),
        );
    }
    model.active_document().text()
}

#[test]
fn a_reserved_word_is_capitalized_once_it_is_finished() {
    assert_eq!(
        typed("select id, status from venda where id in (1) order by id desc;"),
        "SELECT id, status FROM venda WHERE id IN (1) ORDER BY id DESC;"
    );
    assert_eq!(typed("select\n"), "SELECT\n");
    assert_eq!(
        typed("select count(*) from t group by a having count(*) > 1 "),
        "SELECT count(*) FROM t GROUP BY a HAVING count(*) > 1 "
    );
}

#[test]
fn a_word_still_being_typed_is_left_alone() {
    assert_eq!(typed("select"), "select");
    assert_eq!(typed("selection "), "selection ");
}

#[test]
fn strings_comments_names_and_parameters_are_left_as_typed() {
    for text in [
        "'select from' ",
        "-- select from where\n",
        "/* order by */ ",
        "t.order ",
        "\"select\" ",
        ":limit ",
        "a_select ",
    ] {
        let out = typed(text);
        assert_eq!(out.to_ascii_lowercase(), out, "{text:?} became {out:?}");
    }
}

/// `status`, `level`, `name` are names in most schemas; only words no table can go by
/// change case.
#[test]
fn names_that_merely_look_like_keywords_stay_lowercase() {
    assert_eq!(
        typed("select status, level, name, data from usuario "),
        "SELECT status, level, name, data FROM usuario "
    );
}

/// `from venda or` offered nothing until Ctrl+Space.
#[test]
fn order_by_comes_up_after_a_table_and_goes_in_capitals() {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    for ch in "select * from venda ord".chars() {
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
        );
    }
    assert!(model.editor.completion_open, "nothing came up");
    assert_eq!(model.editor.completions[0].label, "ORDER BY");
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert_eq!(
        model.active_document().text(),
        "SELECT * FROM venda ORDER BY"
    );
}
