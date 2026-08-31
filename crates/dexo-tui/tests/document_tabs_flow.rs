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

fn dirty_file_document() -> Model {
    let mut model = Model::default();
    model.documents[0].path = Some("query.sql".into());
    model
        .documents
        .push(EditorDocument::new_unique("q2.sql", None, None));
    model.active_document = 0;
    model
        .active_document_mut()
        .sql
        .insert(0, "select 1")
        .unwrap();
    model
}

#[test]
fn closing_dirty_file_document_waits_for_the_save_to_land() {
    let mut model = dirty_file_document();
    let id = model.active_document().id.clone();

    let effects = update(&mut model, Action::CloseDocument);
    let revision = effects
        .iter()
        .find_map(|effect| match effect {
            dexo_tui::Effect::SaveDocument(request) => Some(request.revision),
            _ => None,
        })
        .expect("closing a dirty file saves it");

    assert_eq!(
        model.documents.len(),
        2,
        "the buffer is the only copy so far"
    );
    assert_eq!(model.active_document().id, id);
    assert!(effects.iter().any(|effect| {
        matches!(
            effect,
            dexo_tui::Effect::SaveDocument(request)
                if request.path == std::path::Path::new("query.sql")
                    && request.content == "select 1"
        )
    }));
    assert_eq!(
        model.messages.last().map(String::as_str),
        Some("Saving dirty file before closing it.")
    );

    update(
        &mut model,
        Action::DocumentSaved {
            document: id,
            revision,
        },
    );

    assert_eq!(model.documents.len(), 1);
    assert_eq!(model.active_document().title, "q2.sql");
}

#[test]
fn stale_save_acknowledgement_does_not_close_newer_dirty_document() {
    let mut model = dirty_file_document();
    let id = model.active_document().id.clone();

    let effects = update(&mut model, Action::CloseDocument);
    let old_revision = effects
        .iter()
        .find_map(|effect| match effect {
            dexo_tui::Effect::SaveDocument(request) => Some(request.revision),
            _ => None,
        })
        .expect("closing a dirty file saves it");

    let cursor = model.active_document().cursor();
    model
        .active_document_mut()
        .sql
        .insert(cursor, " -- newer")
        .unwrap();
    let text = model.active_document().text();
    let current_revision = model.active_document().sql.revision();
    assert!(current_revision > old_revision);

    update(
        &mut model,
        Action::DocumentSaved {
            document: id.clone(),
            revision: old_revision,
        },
    );

    assert_eq!(model.documents.len(), 2);
    assert_eq!(model.active_document().id, id);
    assert_eq!(model.active_document().text(), text);
    assert!(model.active_document().is_dirty());
    assert!(model.pending_document_close.is_none());

    let effects = update(&mut model, Action::CloseDocument);
    let current_save_revision = effects
        .iter()
        .find_map(|effect| match effect {
            dexo_tui::Effect::SaveDocument(request) => Some(request.revision),
            _ => None,
        })
        .expect("closing the newer dirty document saves it");
    assert_eq!(current_save_revision, current_revision);

    update(
        &mut model,
        Action::DocumentSaved {
            document: id,
            revision: current_save_revision,
        },
    );

    assert_eq!(model.documents.len(), 1);
    assert_eq!(model.active_document().title, "q2.sql");
}

#[test]
fn a_failed_save_keeps_the_dirty_tab_open() {
    let mut model = dirty_file_document();

    update(&mut model, Action::CloseDocument);
    update(
        &mut model,
        Action::DocumentConflict {
            path: "query.sql".into(),
        },
    );

    assert_eq!(model.documents.len(), 2);
    assert!(model.active_document().is_dirty());
    assert_eq!(model.active_document().text(), "select 1");
}

#[test]
fn a_pathless_dirty_document_still_checkpoints_to_recovery() {
    let mut model = Model::default();
    model
        .active_document_mut()
        .sql
        .insert(0, "select 1")
        .unwrap();

    let effects = update(&mut model, Action::CheckpointTick);

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::CheckpointRecovery(_))),
        "{effects:?}"
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
