use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::{Effect, Focus, Model, update};

fn editor_with(text: &str, cursor: usize) -> Model {
    let mut model = Model {
        focus: Focus::Editor,
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

fn copied(effects: &[Effect]) -> Option<&str> {
    effects.iter().find_map(|effect| match effect {
        Effect::CopyToClipboard { text } => Some(text.as_str()),
        _ => None,
    })
}

/// Ctrl+Backspace takes back exactly what Ctrl+Left would cross, so deleting and
/// moving by word agree.
#[test]
fn ctrl_backspace_deletes_the_word_ctrl_left_would_cross() {
    let text = "select name from orders";
    for (code, modifiers) in [
        (KeyCode::Backspace, KeyModifiers::CONTROL),
        (KeyCode::Backspace, KeyModifiers::ALT),
        // Terminals without the extended keyboard protocol send Ctrl+Backspace as ^H.
        (KeyCode::Char('h'), KeyModifiers::CONTROL),
    ] {
        let mut model = editor_with(text, text.len());
        press(&mut model, code, modifiers);
        assert_eq!(
            model.active_document().text(),
            "select name from ",
            "{code:?}"
        );
    }

    let mut model = editor_with(text, text.len());
    press(&mut model, KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(model.active_document().text(), "select name from order");
}

#[test]
fn ctrl_delete_deletes_the_word_ahead() {
    let mut model = editor_with("select name from orders", 7);
    press(&mut model, KeyCode::Delete, KeyModifiers::CONTROL);
    assert_eq!(model.active_document().text(), "select from orders");
}

#[test]
fn ctrl_delete_takes_blanks_on_one_side_of_the_word() {
    let mut model = editor_with("foo bar baz", 3);
    press(&mut model, KeyCode::Delete, KeyModifiers::CONTROL);
    assert_eq!(model.active_document().text(), "foo baz");
}

/// Ctrl+Left/Right stop at a line's edges and at an empty line instead of running on to
/// the next word, cross a lone `.` with its word, and keep `ç` inside its word.
#[test]
fn ctrl_arrows_stop_at_line_edges_and_keep_accented_words_whole() {
    let text = "select p.preço\n\n    from produtos";
    let walk = |code: KeyCode, from: usize, presses: usize| {
        let mut model = editor_with(text, from);
        (0..presses)
            .map(|_| {
                press(&mut model, code, KeyModifiers::CONTROL);
                model.active_document().cursor()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(walk(KeyCode::Right, 0, 7), [6, 8, 14, 15, 24, 33, 33]);
    assert_eq!(walk(KeyCode::Left, 33, 8), [25, 20, 16, 15, 9, 7, 0, 0]);
}

#[test]
fn a_word_delete_is_one_undo_step_and_a_selection_goes_first() {
    let mut model = editor_with("select name from orders", 23);
    press(&mut model, KeyCode::Backspace, KeyModifiers::CONTROL);
    press(&mut model, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(model.active_document().text(), "select name from orders");

    let mut model = editor_with("select name from orders", 11);
    model.active_document_mut().anchor = Some(7);
    press(&mut model, KeyCode::Backspace, KeyModifiers::CONTROL);
    assert_eq!(model.active_document().text(), "select  from orders");
}

#[test]
fn ctrl_c_copies_the_selection() {
    let mut model = editor_with("select name from orders", 11);
    model.active_document_mut().anchor = Some(7);
    let effects = press(&mut model, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(copied(&effects), Some("name"));
    assert_eq!(model.active_document().text(), "select name from orders");
}

/// With nothing selected Ctrl+C takes the line under the cursor, newline included, and
/// Ctrl+X removes it -- the VS Code rule.
#[test]
fn without_a_selection_copy_and_cut_take_the_whole_line() {
    let text = "select 1;\nselect 2;\nselect 3;";
    let mut model = editor_with(text, 12);
    let effects = press(&mut model, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(copied(&effects), Some("select 2;\n"));

    let effects = press(&mut model, KeyCode::Char('x'), KeyModifiers::CONTROL);
    assert_eq!(copied(&effects), Some("select 2;\n"));
    assert_eq!(model.active_document().text(), "select 1;\nselect 3;");
}
