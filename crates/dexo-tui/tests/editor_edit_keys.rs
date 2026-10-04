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
fn ctrl_up_and_down_scroll_the_view_and_leave_the_cursor() {
    let mut model = editor_with("a\nb\nc", 0);
    for _ in 0..5 {
        press(&mut model, KeyCode::Down, KeyModifiers::CONTROL);
    }
    assert_eq!(model.active_document().viewport_line, 2);
    press(&mut model, KeyCode::Up, KeyModifiers::CONTROL);
    assert_eq!(model.active_document().viewport_line, 1);
    assert_eq!(model.active_document().cursor(), 0);
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

/// Each line edit is one undo step and counts in chars, so accented text survives.
#[test]
fn line_edits_comment_duplicate_and_move_in_one_undo_step() {
    let text = "select ação\n  from t\nwhere x";
    let mut model = editor_with(text, 2);
    update(&mut model, Action::EditorToggleComment);
    assert_eq!(
        model.active_document().text(),
        "-- select ação\n  from t\nwhere x"
    );
    assert_eq!(model.active_document().cursor(), 5);
    update(&mut model, Action::EditorToggleComment);
    assert_eq!(model.active_document().text(), text);

    // A block: every touched line, the dashes at its shallowest indent.
    let mut model = editor_with(text, 0);
    model.active_document_mut().anchor = Some(3);
    model.active_document_mut().sql.set_cursor(15).unwrap();
    update(&mut model, Action::EditorToggleComment);
    assert_eq!(
        model.active_document().text(),
        "-- select ação\n--   from t\nwhere x"
    );
    press(&mut model, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(model.active_document().text(), text);

    let mut model = editor_with(text, 14);
    update(&mut model, Action::EditorDuplicateLine);
    assert_eq!(
        model.active_document().text(),
        "select ação\n  from t\n  from t\nwhere x"
    );
    update(&mut model, Action::EditorMoveLine { up: true });
    update(&mut model, Action::EditorMoveLine { up: true });
    assert_eq!(
        model.active_document().text(),
        "  from t\nselect ação\n  from t\nwhere x"
    );
    // Nothing above the first line to trade with.
    update(&mut model, Action::EditorMoveLine { up: true });
    assert_eq!(model.active_document().cursor(), 2);
    press(&mut model, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(
        model.active_document().text(),
        "select ação\n  from t\n  from t\nwhere x"
    );
}

/// What the external editor saved replaces the document as one undo step, without the
/// line break editors add at the end; a failed edit leaves it alone.
#[test]
fn an_external_edit_is_one_undo_step() {
    let mut model = editor_with("select 1;", 9);
    let document = model.active_document().id.clone();
    update(
        &mut model,
        Action::ExternalEditFinished {
            document: document.clone(),
            text: Err("vi exited with 1".into()),
        },
    );
    assert_eq!(model.active_document().text(), "select 1;");
    update(
        &mut model,
        Action::ExternalEditFinished {
            document,
            text: Ok("-- ação\nselect 1;\n".into()),
        },
    );
    assert_eq!(model.active_document().text(), "-- ação\nselect 1;");
    press(&mut model, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(model.active_document().text(), "select 1;");
}

/// A cursor in the indentation stays there when the line is commented.
#[test]
fn toggling_a_comment_leaves_a_cursor_in_the_indent() {
    let mut model = editor_with("    select 1", 1);
    update(&mut model, Action::EditorToggleComment);
    assert_eq!(model.active_document().text(), "    -- select 1");
    assert_eq!(model.active_document().cursor(), 1);
    update(&mut model, Action::EditorToggleComment);
    assert_eq!(model.active_document().cursor(), 1);
}

/// A letter with Alt or Ctrl is a command: where nothing is bound to it, it does nothing
/// instead of being typed (Alt+J, Alt+Z and Alt+F left `jzf` in the document).
#[test]
fn an_unbound_alt_letter_is_not_typed() {
    let mut model = editor_with("select 1", 8);
    for letter in ['j', 'z', 'f'] {
        press(&mut model, KeyCode::Char(letter), KeyModifiers::ALT);
    }
    assert_eq!(model.active_document().text(), "select 1");
    press(&mut model, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(model.active_document().text(), "select 1j");
}

/// Select All on an empty document left a selection that began with the first letter
/// typed, and the second letter replaced it: `abc` came out as `bc`.
#[test]
fn select_all_in_an_empty_document_does_not_eat_the_first_letter() {
    let mut model = editor_with("", 0);
    press(&mut model, KeyCode::Char('a'), KeyModifiers::CONTROL);
    for letter in ['a', 'b', 'c'] {
        press(&mut model, KeyCode::Char(letter), KeyModifiers::NONE);
    }
    assert_eq!(model.active_document().text(), "abc");
}
