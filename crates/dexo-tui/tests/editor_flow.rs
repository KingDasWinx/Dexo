use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::update;

fn key(code: KeyCode) -> Action {
    Action::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> Action {
    Action::Key(KeyEvent::new(code, modifiers))
}

fn ctrl(ch: char) -> Action {
    key_mod(KeyCode::Char(ch), KeyModifiers::CONTROL)
}

fn send_text(model: &mut Model, text: &str) {
    for ch in text.chars() {
        update(model, key(KeyCode::Char(ch)));
    }
}

#[test]
fn editor_types_unicode_moves_and_undoes() {
    let mut model = Model::default();
    send_text(&mut model, "select 'ação'");
    assert_eq!(model.active_document().text(), "select 'ação'");
    update(&mut model, ctrl('z'));
    assert_eq!(model.active_document().text(), "");
}

#[test]
fn editor_arrows_home_end_and_word_motion() {
    let mut model = Model::default();
    send_text(&mut model, "select from users");
    update(&mut model, key(KeyCode::Home));
    assert_eq!(model.active_document().cursor(), 0);
    update(&mut model, key(KeyCode::End));
    assert_eq!(
        model.active_document().cursor(),
        "select from users".chars().count()
    );
    update(&mut model, key_mod(KeyCode::Left, KeyModifiers::CONTROL));
    assert_eq!(model.active_document().cursor(), "select from ".chars().count());
    update(&mut model, key(KeyCode::Left));
    assert_eq!(
        model.active_document().cursor(),
        "select from".chars().count()
    );
    update(&mut model, key(KeyCode::Right));
    update(&mut model, key_mod(KeyCode::Right, KeyModifiers::CONTROL));
    assert_eq!(
        model.active_document().cursor(),
        "select from users".chars().count()
    );
}

#[test]
fn editor_backspace_delete_and_shift_selection() {
    let mut model = Model::default();
    send_text(&mut model, "abcd");
    update(&mut model, key(KeyCode::Left));
    update(&mut model, key(KeyCode::Backspace));
    assert_eq!(model.active_document().text(), "abd");
    update(&mut model, key(KeyCode::Home));
    update(&mut model, key(KeyCode::Delete));
    assert_eq!(model.active_document().text(), "bd");
    update(&mut model, key_mod(KeyCode::Right, KeyModifiers::SHIFT));
    send_text(&mut model, "x");
    assert_eq!(model.active_document().text(), "xd");
}

#[test]
fn editor_select_all_indent_tab_and_redo() {
    let mut model = Model::default();
    send_text(&mut model, "select 1");
    update(&mut model, ctrl('a'));
    assert_eq!(
        model.active_document().selection(),
        Some(0.."select 1".chars().count())
    );
    update(&mut model, key(KeyCode::End));
    update(&mut model, key(KeyCode::Enter));
    assert_eq!(model.active_document().text(), "select 1\n");
    send_text(&mut model, "  two");
    update(&mut model, key(KeyCode::Enter));
    assert_eq!(model.active_document().text(), "select 1\n  two\n  ");
    update(&mut model, key(KeyCode::Tab));
    assert_eq!(model.active_document().text(), "select 1\n  two\n      ");
    update(&mut model, ctrl('z'));
    update(&mut model, ctrl('y'));
    assert_eq!(model.active_document().text(), "select 1\n  two\n      ");
}

#[test]
fn editor_scrolls_cursor_into_view() {
    let mut model = Model::default();
    for _ in 0..20 {
        update(&mut model, key(KeyCode::Enter));
    }
    let doc = model.active_document();
    let line = doc.text().matches('\n').count();
    assert!(doc.viewport_line > 0, "viewport should follow cursor");
    assert!(line >= doc.viewport_line);
    assert!(line < doc.viewport_line + 12);
}
