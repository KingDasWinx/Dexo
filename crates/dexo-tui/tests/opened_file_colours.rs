//! A file opened from Open file comes up coloured. Its tab was made empty and painted so,
//! then its text arrived with the same revision, 0: the empty paint passed for current,
//! and the SQL showed uncoloured until it was edited.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::{Action, Effect};
use dexo_tui::screens::file_picker::FilePickerMode;
use dexo_tui::{Model, update};

#[test]
fn a_file_opened_from_the_picker_is_coloured_when_it_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("orders.sql");
    std::fs::write(&path, "select id from orders").unwrap();
    let mut model = Model::default();
    model.apply_size(120, 40);
    model.file_picker.cwd = dir.path().to_path_buf();
    model.file_picker.finding = true;
    model.file_picker_mode = FilePickerMode::Open;
    model.file_picker.open_browser();
    for ch in "orders".chars() {
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
        );
    }
    let effects = update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    let Some(Effect::LoadDocument(request)) = effects
        .into_iter()
        .find(|effect| matches!(effect, Effect::LoadDocument(_)))
    else {
        panic!("no file was asked for");
    };

    update(
        &mut model,
        Action::DocumentLoaded {
            document: request.document,
            path,
            content: "select id from orders".into(),
        },
    );

    assert_eq!(model.active_document().text(), "select id from orders");
    assert!(
        !model.editor.highlights.is_empty(),
        "the SQL came up uncoloured"
    );
    assert!(
        model
            .editor
            .highlights
            .iter()
            .all(|span| span.byte_range.end <= "select id from orders".len()),
        "the colours are the empty tab's"
    );
}
