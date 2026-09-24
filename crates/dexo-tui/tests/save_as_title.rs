use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::screens::file_picker::FilePickerFocus;
use dexo_tui::{Effect, Focus, Model, update};

fn press(model: &mut Model, code: KeyCode, modifiers: KeyModifiers) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, modifiers)))
}

/// Saving an untitled `query-1.sql` as `save2.sql` kept the old name on the tab and
/// the editor title, though the file on disk was `save2.sql`.
#[test]
fn save_as_names_the_tab_after_the_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    model.active_document_mut().title = "query-1.sql".into();
    model
        .active_document_mut()
        .sql
        .insert(0, "select 1")
        .unwrap();

    press(&mut model, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(
        model.file_picker.open,
        "Ctrl+S on an untitled document asks where"
    );
    model.file_picker.cwd = dir.path().to_path_buf();
    model.file_picker.focus = FilePickerFocus::Name;
    model.file_picker.name.set_text("save2.sql");
    let effects = press(&mut model, KeyCode::Enter, KeyModifiers::NONE);

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::SaveDocument(_))),
        "{effects:?}"
    );
    let document = model.active_document();
    assert_eq!(document.title, "save2.sql");
    assert_eq!(
        document.path.as_deref(),
        Some(dir.path().join("save2.sql").as_path())
    );
    let frame = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(!frame.contains("query-1.sql"), "{frame}");
}

/// A document with a file keeps the name F2 gave it when saved again in place.
#[test]
fn a_plain_save_keeps_a_renamed_title() {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    let doc = model.active_document_mut();
    doc.path = Some("/tmp/report.sql".into());
    doc.title = "monthly report".into();
    doc.sql.insert(0, "select 1").unwrap();

    press(&mut model, KeyCode::Char('s'), KeyModifiers::CONTROL);

    assert!(!model.file_picker.open);
    assert_eq!(model.active_document().title, "monthly report");
}
