use dexo_tui::action::Action;
use dexo_tui::model::EditorDocument;
use dexo_tui::{Model, update};

/// Every highlight span must name text the active document really holds at its range.
fn highlights_match_the_active_document(model: &Model) -> bool {
    let text = model.active_document().text();
    model.editor.highlights.iter().all(|span| {
        text.get(span.byte_range.clone())
            .is_some_and(|slice| slice == span.text)
    })
}

/// Switching tabs painted the previous document's spans over the new one until the
/// next edit.
#[test]
fn switching_tabs_repaints_for_the_new_document() {
    let mut model = Model::default();
    model
        .active_document_mut()
        .sql
        .insert(0, "select id, name from orders where id = 1")
        .unwrap();
    let mut other = EditorDocument::new_unique("other.sql", None, None);
    other
        .sql
        .insert(0, "-- a comment\nupdate t set a = 1")
        .unwrap();
    model.documents.push(other);
    update(&mut model, Action::SelectDocument { index: 0 });
    assert!(highlights_match_the_active_document(&model));

    update(&mut model, Action::SelectDocument { index: 1 });

    assert!(
        highlights_match_the_active_document(&model),
        "spans from the previous tab: {:?}",
        model.editor.highlights.first()
    );
    assert!(
        model
            .editor
            .highlights
            .iter()
            .any(|span| span.text == "update"),
        "the new tab is not highlighted"
    );
}
