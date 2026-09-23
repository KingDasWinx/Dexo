//! The tab strip is a pane. Its keys used to be special cases in `handle_key`, guarded
//! on the editor having focus, so Enter on `+` worked from one pane and did nothing
//! from another, and Ctrl+N meant two different things depending on where you stood.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers as M};
use dexo_tui::action::Action;
use dexo_tui::model::{DocumentTabFocus, Focus, Model};
use dexo_tui::update::update;

fn key(code: KeyCode, modifiers: M) -> Action {
    Action::Key(KeyEvent::new(code, modifiers))
}

fn workbench(focus: Focus) -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.focus = focus;
    model
}

/// The reported bug: from the results pane, Enter on `+` did nothing at all.
#[test]
fn enter_on_the_plus_starts_a_document_from_every_pane() {
    for focus in [Focus::Editor, Focus::Results, Focus::Explorer] {
        let mut model = workbench(focus);
        // Alt+0 reaches the strip from anywhere; Alt+Right only where the pane in front
        // does not already own it, which the explorer does for its own width.
        update(&mut model, key(KeyCode::Char('0'), M::ALT));
        update(&mut model, key(KeyCode::Right, M::NONE));
        assert_eq!(model.document_tab_focus, DocumentTabFocus::New);
        assert_eq!(
            model.focus,
            Focus::DocumentTabs,
            "walking the strip from {focus:?} did not enter it"
        );

        update(&mut model, key(KeyCode::Enter, M::NONE));
        assert!(
            model.document_name_prompt.open,
            "Enter on + did nothing coming from {focus:?}"
        );
    }
}

/// The other half: Ctrl+N built a grid row instead of a document whenever the results
/// pane was the one in front.
#[test]
fn ctrl_n_means_one_thing_everywhere() {
    for focus in [Focus::Editor, Focus::Results, Focus::Explorer] {
        let mut model = workbench(focus);
        update(&mut model, key(KeyCode::Char('n'), M::CONTROL));
        assert!(
            model.document_name_prompt.open,
            "Ctrl+N from {focus:?} did not start a document"
        );
        assert!(
            !model.data.insert_form.open,
            "Ctrl+N from {focus:?} opened a grid row"
        );
    }
}

/// The grid still inserts rows; the action kept its place and gave up the chord.
#[test]
fn the_grid_still_inserts_a_row_on_i() {
    let mut model = workbench(Focus::Results);
    model.active_session = Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1)));
    model.session_generation = 1;
    update(&mut model, key(KeyCode::Char('i'), M::NONE));
    assert!(model.data.insert_form.open, "`i` no longer inserts a row");
}

#[test]
fn enter_on_a_tab_hands_focus_to_the_document() {
    let mut model = workbench(Focus::Results);
    model
        .documents
        .push(dexo_tui::model::EditorDocument::new_unique(
            "second.sql",
            None,
            None,
        ));
    update(&mut model, key(KeyCode::Char('0'), M::ALT));
    update(&mut model, key(KeyCode::Right, M::NONE));
    assert_eq!(model.document_tab_focus, DocumentTabFocus::Document(1));

    update(&mut model, key(KeyCode::Enter, M::NONE));
    assert_eq!(model.focus, Focus::Editor);
    assert_eq!(model.active_document, 1);
}

#[test]
fn esc_leaves_the_strip_for_the_editor() {
    let mut model = workbench(Focus::Results);
    update(&mut model, key(KeyCode::Char('0'), M::ALT));
    assert_eq!(model.focus, Focus::DocumentTabs);
    update(&mut model, key(KeyCode::Esc, M::NONE));
    assert_eq!(model.focus, Focus::Editor);
}

#[test]
fn alt_zero_focuses_the_strip() {
    let mut model = workbench(Focus::Results);
    update(&mut model, key(KeyCode::Char('0'), M::ALT));
    assert_eq!(model.focus, Focus::DocumentTabs);
}

/// Typing while the strip has focus must not reach the buffer -- that reach is what the
/// removed special case was papering over.
#[test]
fn typing_in_the_strip_does_not_reach_the_buffer() {
    let mut model = workbench(Focus::Editor);
    update(&mut model, key(KeyCode::Char('0'), M::ALT));
    let before = model.active_document().text();
    update(&mut model, key(KeyCode::Char('x'), M::NONE));
    assert_eq!(model.active_document().text(), before);
}

/// A focused pane has to look focused, and it has to do so without colour: the cursor
/// is bracketed, spending the spaces the label already carries rather than a cell.
#[test]
fn the_focused_strip_brackets_its_cursor() {
    let mut model = workbench(Focus::Editor);
    model
        .documents
        .push(dexo_tui::model::EditorDocument::new_unique(
            "query-1.sql",
            None,
            None,
        ));
    let unfocused = dexo_tui::render::render_to_string(&model, 100, 14);
    assert!(
        !unfocused.contains("[+]"),
        "the strip looks focused when it is not"
    );

    update(&mut model, key(KeyCode::Char('0'), M::ALT));
    let view = dexo_tui::render::render_to_string(&model, 100, 14);
    assert!(
        view.contains("[scratch.sql]"),
        "the cursor is invisible without colour:\n{view}"
    );

    update(&mut model, key(KeyCode::Right, M::NONE));
    update(&mut model, key(KeyCode::Right, M::NONE));
    let view = dexo_tui::render::render_to_string(&model, 100, 14);
    assert!(
        view.contains("[+]"),
        "the new-document slot is not marked:\n{view}"
    );
}
