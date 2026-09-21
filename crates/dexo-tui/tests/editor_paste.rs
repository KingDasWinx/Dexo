//! Bracketed paste. Without it a paste arrives as the keys it happens to look like:
//! every character dispatched and redrawn on its own, and every tab in the text firing
//! whatever tab is bound to -- which, with the completion popup the typing had just
//! opened, meant accepting a suggestion instead of indenting.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers as M};
use dexo_tui::action::Action;
use dexo_tui::event::action_from_event;
use dexo_tui::model::{Focus, Model};
use dexo_tui::update::update;

const INDENTED: &str = "select\n\tid,\n\tname\nfrom users\nwhere id = 1;";

fn editor() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.focus = Focus::Editor;
    model
}

#[test]
fn a_paste_event_becomes_a_paste_action() {
    assert_eq!(
        action_from_event(Event::Paste("select 1".into())),
        Some(Action::Paste("select 1".into())),
        "the paste fell through and was left to arrive as keys"
    );
}

#[test]
fn pasted_tabs_stay_tabs() {
    let mut model = editor();
    update(&mut model, Action::Paste(INDENTED.into()));
    assert_eq!(
        model.active_document().text(),
        INDENTED,
        "the pasted text is not what was pasted"
    );
}

/// The failure that made indentation unrecoverable: a tab arriving as a key while the
/// popup is open accepts a completion. A paste must not be able to reach that.
#[test]
fn a_paste_does_not_accept_an_open_completion() {
    let mut model = editor();
    model.editor.completion_open = true;
    update(&mut model, Action::Paste("\tid".into()));
    assert!(
        !model.editor.completion_open,
        "the popup survived the paste"
    );
    assert_eq!(model.active_document().text(), "\tid");
}

/// One undo step, not one per character.
#[test]
fn a_paste_undoes_in_one_step() {
    let mut model = editor();
    update(&mut model, Action::Paste(INDENTED.into()));
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('z'), M::CONTROL)),
    );
    assert_eq!(
        model.active_document().text(),
        "",
        "undo left part of the paste behind"
    );
}

#[test]
fn carriage_returns_do_not_survive_the_trip() {
    let mut model = editor();
    update(&mut model, Action::Paste("a\r\nb\rc".into()));
    assert_eq!(model.active_document().text(), "a\nb\nc");
}

/// Outside the editor the widgets only know keys, and the text is short.
#[test]
fn pasting_into_a_form_field_still_types() {
    let mut model = editor();
    update(&mut model, Action::OpenConnectionForm);
    assert!(model.connection_form.open);
    let before = model.connection_form.fields[model.connection_form.focus]
        .value
        .clone();
    update(&mut model, Action::Paste("host1".into()));
    assert_ne!(
        model.connection_form.fields[model.connection_form.focus].value, before,
        "the paste did nothing in a form field"
    );
}
