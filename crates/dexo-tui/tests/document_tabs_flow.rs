use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::model::EditorDocument;
use dexo_tui::{Action, Focus, Model, update};

fn two_documents() -> Model {
    let mut model = Model::from(Focus::Explorer);
    model
        .documents
        .push(EditorDocument::new_unique("q2.sql", None, None));
    model
}

#[test]
fn selecting_and_cycling_documents_focuses_the_editor() {
    let mut model = two_documents();

    update(&mut model, Action::SelectDocument { index: 1 });
    update(&mut model, Action::PrevDocument);
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL)),
    );

    assert_eq!(model.active_document, 1);
    assert_eq!(model.focus, Focus::Editor);
}

#[test]
fn closing_dirty_untitled_document_keeps_it_open() {
    let mut model = Model::default();
    model
        .active_document_mut()
        .sql
        .insert(0, "select 1")
        .unwrap();

    update(&mut model, Action::CloseDocument);

    assert_eq!(model.documents.len(), 1);
    assert!(model.active_document().is_dirty());
    assert_eq!(
        model.messages.last().map(String::as_str),
        Some("Save the untitled document before closing it.")
    );
}

#[test]
fn closing_dirty_file_document_removes_it_with_autosave_notice() {
    let mut model = Model::default();
    model.documents[0].path = Some("query.sql".into());
    model
        .active_document_mut()
        .sql
        .insert(0, "select 1")
        .unwrap();

    let effects = update(&mut model, Action::CloseDocument);

    assert_eq!(model.documents.len(), 1);
    assert!(!model.active_document().is_dirty());
    assert!(effects.iter().any(|effect| {
        matches!(
            effect,
            dexo_tui::Effect::SaveDocument(request)
                if request.path == std::path::PathBuf::from("query.sql")
                    && request.content == "select 1"
        )
    }));
    assert_eq!(
        model.messages.last().map(String::as_str),
        Some("Saving dirty file before closing it.")
    );
}

#[test]
fn compact_sql_workbench_renders_document_tabs() {
    let mut model = Model::default();
    model
        .documents
        .push(EditorDocument::new_unique("q2.sql", None, None));

    let frame = dexo_tui::render::render_to_string(&model, 60, 20);

    assert!(frame.contains("q2.sql"));
}
