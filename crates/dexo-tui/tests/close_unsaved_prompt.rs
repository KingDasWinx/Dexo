use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_tui::model::{CloseChoice, EditorDocument};
use dexo_tui::mouse::{HitButton, HitMap, HitTarget};
use dexo_tui::{Action, Effect, Model, update};

fn key(code: KeyCode) -> Action {
    Action::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Two tabs, the first a dirty file so closing it has something to lose.
fn dirty_first_tab() -> Model {
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
fn a_clean_document_closes_without_asking() {
    let mut model = dirty_first_tab();
    model.active_document = 1;

    update(&mut model, Action::CloseDocument);

    assert!(model.close_prompt.is_none());
    assert_eq!(model.documents.len(), 1);
}

#[test]
fn closing_a_dirty_document_asks_with_save_preselected() {
    let mut model = dirty_first_tab();

    let effects = update(&mut model, Action::CloseDocument);

    assert!(effects.is_empty(), "asking must not save yet: {effects:?}");
    let prompt = model.close_prompt.as_ref().expect("closing did not ask");
    assert_eq!(prompt.choice, CloseChoice::Save);
    assert_eq!(model.documents.len(), 2);
    let frame = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(frame.contains("Unsaved changes"), "{frame}");
    assert!(frame.contains("[Don't save]"), "{frame}");
}

#[test]
fn escape_backs_out_and_keeps_the_text() {
    let mut model = dirty_first_tab();
    update(&mut model, Action::CloseDocument);

    update(&mut model, key(KeyCode::Esc));

    assert!(model.close_prompt.is_none());
    assert_eq!(model.documents.len(), 2);
    assert_eq!(model.active_document().text(), "select 1");
}

/// The prompt eats keys while open; a stray letter must not land in the buffer.
#[test]
fn keys_do_not_reach_the_editor_while_asking() {
    let mut model = dirty_first_tab();
    update(&mut model, Action::CloseDocument);

    update(&mut model, key(KeyCode::Char('x')));

    assert!(model.close_prompt.is_some());
    assert_eq!(model.active_document().text(), "select 1");
}

#[test]
fn dont_save_closes_and_drops_the_recovery_checkpoint() {
    let mut model = dirty_first_tab();
    let id = model.active_document().id.clone();
    update(&mut model, Action::CloseDocument);

    update(&mut model, key(KeyCode::Right));
    let effects = update(&mut model, key(KeyCode::Enter));

    assert!(model.close_prompt.is_none());
    assert_eq!(model.documents.len(), 1);
    assert_eq!(model.active_document().title, "q2.sql");
    assert!(
        effects.iter().any(
            |effect| matches!(effect, Effect::DiscardRecovery { document } if *document == id)
        ),
        "{effects:?}"
    );
}

#[test]
fn dont_save_on_the_last_document_leaves_nothing_open() {
    let mut model = Model::default();
    model
        .active_document_mut()
        .sql
        .insert(0, "select 1")
        .unwrap();
    update(&mut model, Action::CloseDocument);

    update(&mut model, key(KeyCode::Char('d')));

    assert!(model.nothing_open());
}

#[test]
fn save_on_an_untitled_document_asks_where_and_backing_out_keeps_it() {
    let mut model = Model::default();
    model
        .active_document_mut()
        .sql
        .insert(0, "select 1")
        .unwrap();
    update(&mut model, Action::CloseDocument);

    update(&mut model, key(KeyCode::Enter));
    assert!(model.file_picker.open, "save did not ask for a path");
    assert!(model.pending_document_close.is_some());

    update(&mut model, key(KeyCode::Esc));

    assert!(!model.file_picker.open);
    assert!(
        model.pending_document_close.is_none(),
        "a later save would close the tab"
    );
    assert_eq!(model.documents.len(), 1);
    assert_eq!(model.active_document().text(), "select 1");
}

#[test]
fn the_buttons_answer_the_mouse() {
    for (button, open_after) in [
        (HitButton::Cancel, 2),
        (HitButton::Discard, 1),
        (HitButton::Confirm, 2),
    ] {
        let mut model = dirty_first_tab();
        update(&mut model, Action::CloseDocument);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        let mut hits = HitMap::default();
        terminal
            .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
            .unwrap();
        model.hits = hits;
        let (column, row) = model.hits.center(HitTarget::Button(button));

        let effects = update(
            &mut model,
            Action::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }),
        );

        assert!(model.close_prompt.is_none(), "{button:?}");
        assert_eq!(model.documents.len(), open_after, "{button:?}");
        if button == HitButton::Confirm {
            assert!(
                effects
                    .iter()
                    .any(|effect| matches!(effect, Effect::SaveDocument(_))),
                "{effects:?}"
            );
        }
    }
}
