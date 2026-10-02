use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::screens::file_picker::{FilePickerFocus, FilePickerMode};
use dexo_tui::{Effect, Model, update};

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

/// A compiled file opened as a document: the read failed with Rust's own words, and the
/// tab it left behind was bound to the file, so one keystroke and Ctrl+S replaced the
/// 2450-byte original with one byte.
#[test]
fn a_file_that_is_not_text_opens_nothing_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("helpdump.pyc");
    std::fs::write(&path, [0xcb, 0x0d, 0x0d, 0x0a, 0xff, 0xfe, 0x00]).unwrap();
    let mut model = Model::default();
    let tabs = model.documents.len();
    model.file_picker_mode = FilePickerMode::Open;
    model.file_picker.open_browser();
    model.file_picker.cwd = dir.path().to_path_buf();
    model.file_picker.name.set_text("helpdump.pyc");
    model.file_picker.focus = FilePickerFocus::Name;

    let effects = press(&mut model, KeyCode::Enter);
    let request = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::LoadDocument(request) => Some(request.clone()),
            _ => None,
        })
        .expect("opening did not load the file");
    let error = std::fs::read_to_string(&path).unwrap_err();
    update(
        &mut model,
        Action::DocumentLoadFailed {
            document: request.document,
            message: dexo_tui::runtime::document_io::load_failure(&request.path, &error),
        },
    );

    assert_eq!(
        model.documents.len(),
        tabs,
        "a tab bound to the file is left"
    );
    assert!(model.recent_sql_files.is_empty());
    let message = &model.messages.last().expect("no message").message;
    assert!(
        message.contains("helpdump.pyc") && message.contains("not a text file"),
        "{message}"
    );
    assert!(!message.contains("stream did not"), "{message}");
}
