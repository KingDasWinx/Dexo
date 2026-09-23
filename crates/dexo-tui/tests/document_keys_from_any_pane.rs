//! Closing, switching and renaming a document are workbench actions, not editor ones.
//! They were bound in `[editor]` only, and a table document puts the grid in the
//! editor's slot -- its effective focus is the results pane -- so Ctrl+W could not close
//! the tab you were looking at, nor could Ctrl+Tab leave it or F2 rename it.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers as M};
use dexo_tui::action::Action;
use dexo_tui::model::{EditorDocument, Focus, Model};
use dexo_tui::update::update;

fn workbench(table: bool, focus: Focus) -> Model {
    let mut model = Model::default();
    model.apply_size(160, 40);
    model.documents.push(if table {
        EditorDocument::new_table(dexo_app::parse_qualified("public.events_1m"), None)
    } else {
        EditorDocument::new_unique("q.sql", None, None)
    });
    model.set_active_document(1);
    model.focus = focus;
    model
}

fn every_pane() -> Vec<(bool, Focus)> {
    vec![
        (false, Focus::Editor),
        (false, Focus::Results),
        (true, Focus::Editor),
        (true, Focus::Results),
        (true, Focus::Console),
    ]
}

#[test]
fn ctrl_w_closes_the_document_from_every_pane() {
    for (table, focus) in every_pane() {
        let mut model = workbench(table, focus);
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('w'), M::CONTROL)),
        );
        assert_eq!(
            model.documents.len(),
            1,
            "Ctrl+W did not close the {} document from {focus:?}",
            if table { "table" } else { "sql" }
        );
    }
}

#[test]
fn ctrl_tab_leaves_the_document_from_every_pane() {
    for (table, focus) in every_pane() {
        let mut model = workbench(table, focus);
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Tab, M::CONTROL)),
        );
        assert_eq!(
            model.active_document, 0,
            "Ctrl+Tab from {focus:?}, table={table}"
        );
    }
}

#[test]
fn f2_renames_the_document_from_every_pane() {
    for (table, focus) in every_pane() {
        let mut model = workbench(table, focus);
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::F(2), M::NONE)),
        );
        assert!(
            model.document_name_prompt.open,
            "F2 from {focus:?}, table={table}"
        );
    }
}
