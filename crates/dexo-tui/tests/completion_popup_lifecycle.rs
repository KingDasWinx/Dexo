use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::model::EditorDocument;
use dexo_tui::{Focus, Model, update};

fn press(model: &mut Model, code: KeyCode, modifiers: KeyModifiers) {
    update(model, Action::Key(KeyEvent::new(code, modifiers)));
}

fn type_text(model: &mut Model, text: &str) {
    for ch in text.chars() {
        press(model, KeyCode::Char(ch), KeyModifiers::NONE);
    }
}

/// An editor with the popup open over `sel`, which completes to `select`.
fn popup_open() -> Model {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    type_text(&mut model, "sel");
    assert!(model.editor.completion_open, "typing `sel` did not open it");
    model
}

/// The popup used to stay up through every cursor move, so the only way out was Esc,
/// and a later Enter wrote at the spot the popup was opened for.
#[test]
fn moving_the_cursor_dismisses_the_popup() {
    for (code, modifiers) in [
        (KeyCode::Left, KeyModifiers::NONE),
        (KeyCode::Right, KeyModifiers::NONE),
        (KeyCode::Home, KeyModifiers::NONE),
        (KeyCode::End, KeyModifiers::NONE),
        (KeyCode::Left, KeyModifiers::CONTROL),
        (KeyCode::Up, KeyModifiers::SHIFT),
    ] {
        let mut model = popup_open();
        press(&mut model, code, modifiers);
        assert!(
            !model.editor.completion_open,
            "{code:?}+{modifiers:?} left it open"
        );
        assert_eq!(model.active_document().text(), "sel");
    }
}

#[test]
fn typing_keeps_it_open_and_the_arrows_walk_it() {
    let mut model = popup_open();
    type_text(&mut model, "e");
    assert!(model.editor.completion_open);

    press(&mut model, KeyCode::Down, KeyModifiers::NONE);
    press(&mut model, KeyCode::Up, KeyModifiers::NONE);
    assert!(model.editor.completion_open, "walking the list closed it");
}

/// Edits that do not recompute the popup leave it answering for text that is gone.
#[test]
fn edits_that_do_not_recompute_dismiss_it() {
    let mut model = popup_open();
    press(&mut model, KeyCode::Left, KeyModifiers::NONE);
    press(&mut model, KeyCode::Char('e'), KeyModifiers::NONE);
    press(&mut model, KeyCode::Delete, KeyModifiers::NONE);
    assert!(!model.editor.completion_open, "Delete left it open");

    let mut model = popup_open();
    press(&mut model, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(!model.editor.completion_open, "undo left it open");
}

#[test]
fn leaving_the_editor_or_the_document_dismisses_it() {
    let mut model = popup_open();
    press(&mut model, KeyCode::Char('1'), KeyModifiers::ALT);
    assert!(
        !model.editor.completion_open,
        "focusing the sidebar left it open"
    );

    let mut model = popup_open();
    model
        .documents
        .push(EditorDocument::new_unique("other.sql", None, None));
    update(&mut model, Action::SelectDocument { index: 1 });
    assert!(
        !model.editor.completion_open,
        "switching documents left it open"
    );

    let mut model = popup_open();
    update(&mut model, Action::OpenPalette);
    assert!(!model.editor.completion_open, "the palette opened over it");
}

/// The popup was laid out without the tab row, one row too high, so its top border
/// sat on the cursor's own line and hid the text to the right of the cursor.
#[test]
fn the_popup_opens_below_the_cursor_line() {
    let model = popup_open();
    let frame = dexo_tui::render::render_to_string(&model, 120, 30);
    let line = frame
        .lines()
        .find(|row| row.contains("1▸sel"))
        .unwrap_or_else(|| panic!("the edited line is not on screen:\n{frame}"));
    assert!(
        !line.contains('┌'),
        "the popup border is drawn on the cursor line:\n{frame}"
    );
    assert!(
        frame.contains("select"),
        "the popup is not on screen:\n{frame}"
    );
}
