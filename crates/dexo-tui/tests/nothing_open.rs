//! Closing the last document used to leave a `scratch.sql` behind -- a document of no
//! connection, which with three connected belonged to nothing in particular. Now the
//! workbench shows that nothing is open, and the first text written becomes a document
//! of the connection it is written for.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers as M};
use dexo_tui::action::Action;
use dexo_tui::model::{DocumentKind, EditorDocument, Focus, Model};
use dexo_tui::update::update;

fn key(code: KeyCode, modifiers: M) -> Action {
    Action::Key(KeyEvent::new(code, modifiers))
}

fn one_document() -> Model {
    let mut model = Model::default();
    model.apply_size(100, 20);
    model.focus = Focus::Editor;
    model.documents = vec![EditorDocument::new_unique("q.sql", None, None)];
    model.set_active_document(0);
    model
}

fn close_everything() -> Model {
    let mut model = one_document();
    update(&mut model, key(KeyCode::Char('w'), M::CONTROL));
    model
}

#[test]
fn closing_the_last_document_leaves_nothing_open() {
    let model = close_everything();
    assert!(model.nothing_open());
    let view = dexo_tui::render::render_to_string(&model, 100, 20);
    assert!(
        !view.contains("scratch.sql"),
        "a scratch document came back:\n{view}"
    );
    assert!(view.contains("No document open"), "{view}");
}

#[test]
fn typing_into_nothing_open_makes_a_document() {
    let mut model = close_everything();
    for ch in "sel".chars() {
        update(&mut model, key(KeyCode::Char(ch), M::NONE));
    }
    assert_eq!(model.active_document().kind, DocumentKind::Console);
    assert_eq!(model.active_document().title, "query-1.sql");
    assert_eq!(model.active_document().text(), "sel");
    assert!(!model.nothing_open());
}

/// The one that can lose work: text can reach the stand-in without a keystroke -- a
/// snippet, a history entry, SQL generated from a result -- and a stand-in still holding
/// it would be filtered out of the next flush.
#[test]
fn text_written_without_a_key_is_never_dropped_on_flush() {
    let mut model = close_everything();
    model.active_document_mut().sql = dexo_sql::SqlDocument::new("select 42");
    let effects = update(&mut model, Action::Quit);
    let flushed = effects
        .iter()
        .find_map(|effect| match effect {
            dexo_tui::Effect::FlushDocuments { documents, .. } => Some(documents),
            _ => None,
        })
        .expect("quitting flushes");
    assert!(
        flushed
            .iter()
            .any(|document| document.content == "select 42"),
        "the text was dropped: {flushed:?}"
    );
}

/// An empty stand-in is not a document, and is not written down.
#[test]
fn nothing_open_is_not_persisted() {
    let mut model = close_everything();
    let effects = update(&mut model, Action::Quit);
    let flushed = effects
        .iter()
        .find_map(|effect| match effect {
            dexo_tui::Effect::FlushDocuments { documents, .. } => Some(documents.len()),
            _ => None,
        })
        .unwrap();
    assert_eq!(flushed, 0, "the stand-in was persisted as a document");
}

/// Every way of opening a real document routes through one guard that drops the
/// stand-in, so none of them leaves a hidden document behind.
#[test]
fn opening_a_document_replaces_nothing_open() {
    let mut model = close_everything();
    update(&mut model, key(KeyCode::Char('n'), M::CONTROL));
    update(&mut model, key(KeyCode::Enter, M::NONE));
    assert_eq!(
        model.documents.len(),
        1,
        "the stand-in stayed beside the new document"
    );
    assert!(!model.active_document().kind.is_placeholder());
}

#[test]
fn the_strip_offers_only_new_when_nothing_is_open() {
    let mut model = close_everything();
    update(&mut model, key(KeyCode::Char('0'), M::ALT));
    for _ in 0..3 {
        update(&mut model, key(KeyCode::Right, M::NONE));
        assert_eq!(
            model.document_tab_focus,
            dexo_tui::model::DocumentTabFocus::New,
            "the strip cursor landed on a tab that is not there"
        );
    }
}

#[test]
fn saving_nothing_open_does_nothing() {
    let mut model = close_everything();
    let effects = update(&mut model, key(KeyCode::Char('s'), M::CONTROL));
    assert!(
        effects.is_empty(),
        "saving nothing asked to save: {effects:?}"
    );
    assert!(
        !model.file_picker.open,
        "saving nothing opened the file picker"
    );
}
