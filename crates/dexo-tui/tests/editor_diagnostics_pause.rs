//! A syntax error by the cursor is held back while typing, and shows once the typing has
//! paused. A mistyped first word is told as soon as the cursor is past it: `selec 1` was
//! left plain however long it sat.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::screens::editor::current_diagnostics;
use dexo_tui::{Action, Focus, Model, update};

fn typed(model: &mut Model, text: &str) {
    for ch in text.chars() {
        update(
            model,
            Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
        );
    }
}

#[test]
fn a_syntax_error_shows_when_the_typing_pauses_and_hides_again_when_it_resumes() {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    typed(&mut model, "select * from orders where (id = 1");
    assert!(
        current_diagnostics(&model).is_empty(),
        "an error by the cursor is not shown while typing"
    );
    assert!(model.editor.hides_errors());

    // The first tick only notes where things stand; the second finds them unchanged.
    update(&mut model, Action::DiagnosticsTick);
    assert!(current_diagnostics(&model).is_empty());
    update(&mut model, Action::DiagnosticsTick);
    let shown = current_diagnostics(&model);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert!(!model.editor.hides_errors());

    // Typing again takes it back off the line until the next pause.
    typed(&mut model, "0");
    assert!(model.editor.hides_errors());
    assert!(current_diagnostics(&model).is_empty());
}

#[test]
fn a_mistyped_keyword_is_underlined_once_the_cursor_is_past_it() {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    typed(&mut model, "selec");
    assert!(
        current_diagnostics(&model).is_empty(),
        "still being typed: it may be on its way to SELECT"
    );
    typed(&mut model, " 1");
    let shown = current_diagnostics(&model);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert!(
        shown[0].message.contains("did you mean SELECT"),
        "{shown:?}"
    );
}

#[test]
fn a_tick_in_the_middle_of_typing_shows_nothing() {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    typed(&mut model, "select * from orders where (id");
    update(&mut model, Action::DiagnosticsTick);
    typed(&mut model, " = 1");
    update(&mut model, Action::DiagnosticsTick);
    assert!(current_diagnostics(&model).is_empty());
}
