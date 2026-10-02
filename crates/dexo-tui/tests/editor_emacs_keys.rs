//! The Emacs profile moves and kills the way Emacs does, and Ctrl+Home / Ctrl+End take
//! any profile to the ends of the document.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::keymap::Keymap;
use dexo_tui::{Action, Effect, Focus, Model, update};

fn editor(text: &str, cursor: usize, keymap: Keymap) -> Model {
    let mut model = Model {
        focus: Focus::Editor,
        keymap,
        ..Model::default()
    };
    let doc = model.active_document_mut();
    doc.sql.insert(0, text).unwrap();
    doc.sql.set_cursor(cursor).unwrap();
    model
}

fn press(model: &mut Model, code: KeyCode, modifiers: KeyModifiers) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, modifiers)))
}

fn ctrl(model: &mut Model, letter: char) {
    press(model, KeyCode::Char(letter), KeyModifiers::CONTROL);
}

#[test]
fn emacs_moves_by_character_line_and_word() {
    let mut model = editor("first line\nsecond line", 15, Keymap::emacs_profile());
    ctrl(&mut model, 'a');
    assert_eq!(model.active_document().cursor(), 11, "C-a: start of line");
    ctrl(&mut model, 'e');
    assert_eq!(model.active_document().cursor(), 22, "C-e: end of line");
    ctrl(&mut model, 'b');
    assert_eq!(model.active_document().cursor(), 21, "C-b");
    ctrl(&mut model, 'f');
    assert_eq!(model.active_document().cursor(), 22, "C-f");
    ctrl(&mut model, 'p');
    assert_eq!(model.active_document().cursor(), 10, "C-p: up a line");
    ctrl(&mut model, 'n');
    assert_eq!(model.active_document().cursor(), 21, "C-n: down a line");
    ctrl(&mut model, 'a');
    press(&mut model, KeyCode::Char('f'), KeyModifiers::ALT);
    assert_eq!(model.active_document().cursor(), 17, "M-f: after the word");
    press(&mut model, KeyCode::Char('b'), KeyModifiers::ALT);
    assert_eq!(model.active_document().cursor(), 11, "M-b: back a word");
    assert_eq!(
        model.active_document().text(),
        "first line\nsecond line",
        "no motion key was typed"
    );
}

#[test]
fn emacs_kills_and_deletes() {
    let mut model = editor("alpha beta\ngamma", 6, Keymap::emacs_profile());
    ctrl(&mut model, 'k');
    assert_eq!(model.active_document().text(), "alpha \ngamma");
    ctrl(&mut model, 'k');
    assert_eq!(
        model.active_document().text(),
        "alpha gamma",
        "C-k at the end joins"
    );
    ctrl(&mut model, 'a');
    ctrl(&mut model, 'd');
    assert_eq!(model.active_document().text(), "lpha gamma", "C-d");
}

/// Ctrl+A moved to the line start, so the way to select everything is C-x h; Ctrl+F is
/// a character right, and Find is Ctrl+S.
#[test]
fn emacs_keeps_select_all_and_find_on_their_own_keys() {
    let mut model = editor("select 1", 0, Keymap::emacs_profile());
    ctrl(&mut model, 'f');
    assert!(!model.find.open);
    ctrl(&mut model, 's');
    assert!(model.find.open);
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    );
    ctrl(&mut model, 'x');
    press(&mut model, KeyCode::Char('h'), KeyModifiers::NONE);
    assert_eq!(model.active_document().selection(), Some(0..8));
}

#[test]
fn ctrl_home_and_ctrl_end_go_to_the_ends_of_the_document() {
    for keymap in [Keymap::default_profile(), Keymap::emacs_profile()] {
        let mut model = editor("one\ntwo\nthree", 5, keymap);
        press(&mut model, KeyCode::End, KeyModifiers::CONTROL);
        assert_eq!(model.active_document().cursor(), 13);
        press(&mut model, KeyCode::Home, KeyModifiers::CONTROL);
        assert_eq!(model.active_document().cursor(), 0);
        press(&mut model, KeyCode::End, KeyModifiers::NONE);
        assert_eq!(
            model.active_document().cursor(),
            3,
            "End is still the line's"
        );
    }
}
