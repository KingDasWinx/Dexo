use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_tui::model::{DocumentTabFocus, EditorDocument};
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::{Action, Focus, Model, update};

fn alt_left() -> Action {
    Action::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))
}

fn alt_right() -> Action {
    Action::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT))
}

fn alt_up() -> Action {
    Action::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT))
}

fn alt_down() -> Action {
    Action::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT))
}

fn enter() -> Action {
    Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

fn confirm_document_name(model: &mut Model) {
    assert!(model.document_name_prompt.open);
    update(model, enter());
}

fn two_documents() -> Model {
    let mut model = Model::from(Focus::Explorer);
    model
        .documents
        .push(EditorDocument::new_unique("q2.sql", None, None));
    model
}

#[test]
fn alt_arrows_cycle_document_tabs_and_the_new_button() {
    let mut model = two_documents();
    model.focus = Focus::Editor;

    update(&mut model, alt_right());

    assert_eq!(model.active_document, 1);
    assert_eq!(model.document_tab_focus, DocumentTabFocus::Document(1));

    update(&mut model, alt_right());

    assert_eq!(model.active_document, 1);
    assert_eq!(model.document_tab_focus, DocumentTabFocus::New);
    assert_eq!(model.focus, Focus::Editor);

    update(&mut model, alt_right());

    assert_eq!(model.active_document, 0);
    assert_eq!(model.document_tab_focus, DocumentTabFocus::Document(0));

    update(&mut model, alt_left());

    assert_eq!(model.document_tab_focus, DocumentTabFocus::New);
}

#[test]
fn alt_arrows_do_not_cycle_document_tabs_outside_the_editor() {
    for focus in [Focus::Explorer, Focus::Results, Focus::Inspector] {
        for action in [alt_left(), alt_right()] {
            let mut model = two_documents();
            model.focus = focus;

            update(&mut model, action);

            assert_eq!(
                model.active_document, 0,
                "active document changed in {focus:?}"
            );
            assert_eq!(
                model.document_tab_focus,
                DocumentTabFocus::Document(0),
                "document tab focus changed in {focus:?}"
            );
            assert_eq!(model.focus, focus, "workbench focus changed in {focus:?}");
        }
    }
}

#[test]
fn alt_arrows_resize_the_focused_side_pane_toward_its_border() {
    let mut model = two_documents();
    model.apply_size(160, 50);

    model.focus = Focus::Explorer;
    let explorer_width = model.panes.explorer_width;
    update(&mut model, alt_right());
    assert_eq!(model.panes.explorer_width, explorer_width + 2);
    update(&mut model, alt_left());
    assert_eq!(model.panes.explorer_width, explorer_width);

    model.focus = Focus::Inspector;
    let inspector_width = model.panes.inspector_width;
    update(&mut model, alt_left());
    assert_eq!(model.panes.inspector_width, inspector_width + 2);
    update(&mut model, alt_right());
    assert_eq!(model.panes.inspector_width, inspector_width);

    model.focus = Focus::Results;
    update(&mut model, alt_left());
    update(&mut model, alt_right());
    assert_eq!(model.panes.explorer_width, explorer_width);
    assert_eq!(model.panes.inspector_width, inspector_width);
}

#[test]
fn alt_up_down_resize_results_height_from_editor_or_results() {
    let mut model = two_documents();
    model.apply_size(160, 50);
    let start = model.panes.results_height;

    model.focus = Focus::Results;
    update(&mut model, alt_up());
    assert_eq!(model.panes.results_height, start + 2);
    update(&mut model, alt_down());
    assert_eq!(model.panes.results_height, start);

    model.focus = Focus::Editor;
    update(&mut model, alt_up());
    assert_eq!(model.panes.results_height, start + 2);
    update(&mut model, alt_down());
    assert_eq!(model.panes.results_height, start);

    model.focus = Focus::Explorer;
    update(&mut model, alt_up());
    update(&mut model, alt_down());
    assert_eq!(model.panes.results_height, start);
}

#[test]
fn document_tab_focus_actions_are_ignored_outside_the_editor() {
    for action in [Action::NextDocumentTabFocus, Action::PrevDocumentTabFocus] {
        let mut model = two_documents();
        let before = model.document_tab_focus;

        update(&mut model, action);

        assert_eq!(model.active_document, 0);
        assert_eq!(model.document_tab_focus, before);
        assert_eq!(model.focus, Focus::Explorer);
    }
}

#[test]
fn enter_on_new_tab_focus_creates_a_document() {
    let mut model = Model::default();
    model.document_tab_focus = DocumentTabFocus::New;

    update(&mut model, enter());
    confirm_document_name(&mut model);

    assert_eq!(model.documents.len(), 2);
    assert_eq!(model.active_document, 1);
    assert_eq!(model.active_document().title, "query-1.sql");
    assert_eq!(model.document_tab_focus, DocumentTabFocus::Document(1));
}

#[test]
fn new_document_prompt_accepts_a_custom_name() {
    let mut model = Model::default();

    update(&mut model, Action::NewDocument);
    assert_eq!(model.document_name_prompt.name.as_str(), "query-1.sql");

    for _ in 0..model.document_name_prompt.name.len() {
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        );
    }
    for ch in "reports".chars() {
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
        );
    }
    confirm_document_name(&mut model);

    assert_eq!(model.documents.len(), 2);
    assert_eq!(model.active_document().title, "reports.sql");
}

#[test]
fn rename_document_prompt_updates_the_active_tab_title() {
    let mut model = Model::default();

    update(&mut model, Action::RenameDocument);
    assert_eq!(model.document_name_prompt.name.as_str(), "scratch.sql");

    for _ in 0..model.document_name_prompt.name.len() {
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        );
    }
    for ch in "daily".chars() {
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
        );
    }
    confirm_document_name(&mut model);

    assert_eq!(model.active_document().title, "daily.sql");
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
fn many_document_tabs_render_with_overflow_indicators() {
    let mut model = Model::default();
    model.documents.clear();
    for index in 0..8 {
        model.documents.push(EditorDocument::new_unique(
            format!("query-{index}.sql"),
            None,
            None,
        ));
    }
    model.active_document = 7;
    model.document_tab_focus = DocumentTabFocus::Document(7);
    model.sync_document_tabs_scroll();

    let frame = dexo_tui::render::render_to_string(&model, 40, 20);

    assert!(frame.contains('‹') || frame.contains('›'), "{frame}");
    assert!(frame.contains('+'), "{frame}");
    assert!(frame.contains("query-7.sql"), "{frame}");
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

#[test]
fn document_tabs_render_distinct_close_and_new_controls() {
    let mut model = Model::default();
    model
        .documents
        .push(EditorDocument::new_unique("q2.sql", None, None));

    let frame = dexo_tui::render::render_to_string(&model, 60, 20);

    assert!(frame.contains("×"), "{frame}");
    assert!(frame.contains("+"), "{frame}");
}

fn click_target(model: &mut Model, target: HitTarget) {
    let (column, row) = model.hits.center(target);
    update(
        model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
}

#[test]
fn clicking_document_tab_close_closes_that_tab() {
    let mut model = two_documents();
    model.active_document = 1;
    let mut hits = HitMap::default();
    hits.register(
        HitTarget::DocumentTabClose(0),
        ratatui::layout::Rect::new(0, 0, 1, 1),
    );
    model.hits = hits;

    click_target(&mut model, HitTarget::DocumentTabClose(0));

    assert_eq!(model.documents.len(), 1);
    assert_eq!(model.active_document().title, "q2.sql");
}

#[test]
fn clicking_document_tab_new_creates_bound_document() {
    let mut model = Model::default();
    let mut hits = HitMap::default();
    hits.register(
        HitTarget::DocumentTabNew,
        ratatui::layout::Rect::new(0, 0, 1, 1),
    );
    model.hits = hits;

    click_target(&mut model, HitTarget::DocumentTabNew);
    confirm_document_name(&mut model);

    assert_eq!(model.documents.len(), 2);
    assert_eq!(model.active_document().title, "query-1.sql");
}
