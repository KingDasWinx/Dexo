use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::screens::file_picker::{FilePickerFocus, FilePickerMode, inner_rows};
use dexo_tui::{Model, update};

fn press(model: &mut Model, code: KeyCode) {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn crowded(dir: &std::path::Path, height: u16) -> Model {
    for i in 0..40 {
        std::fs::write(dir.join(format!("file-{i:02}.sql")), "select 1").unwrap();
    }
    let recent: Vec<_> = (0..8)
        .map(|i| dir.join(format!("recent-{i}.sql")))
        .collect();
    let mut model = Model {
        width: 80,
        height,
        file_picker_mode: FilePickerMode::Open,
        ..Model::default()
    };
    model.file_picker.open = true;
    model.file_picker.cwd = dir.to_path_buf();
    model.file_picker.open_browser_with_recents(&recent);
    model.file_picker.fit_recents(inner_rows(height));
    model
}

/// With recent files and a long folder the list filled the dialog and pushed the name
/// field and the buttons out of it, out of the mouse's reach too.
#[test]
fn the_name_and_the_buttons_stay_in_the_dialog_over_a_long_list() {
    let dir = tempfile::tempdir().unwrap();
    let model = crowded(dir.path(), 24);

    let frame = dexo_tui::render::render_to_string(&model, 80, 24);

    for wanted in ["name:", "[Open]", "[Cancel]", "Esc cancel", "Recent files"] {
        assert!(
            frame.contains(wanted),
            "{wanted} is not on screen:\n{frame}"
        );
    }
}

#[test]
fn a_short_terminal_gives_the_recent_files_up_for_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let model = crowded(dir.path(), 14);

    let frame = dexo_tui::render::render_to_string(&model, 80, 14);

    assert!(!frame.contains("Recent files"), "{frame}");
    assert!(frame.contains("[Open]"), "{frame}");
}

/// The path was cut on the right, so every folder showed the same first 70 characters.
#[test]
fn a_long_path_shows_the_folder_the_user_is_in() {
    let dir = tempfile::tempdir().unwrap();
    let mut deep = dir.path().to_path_buf();
    for level in 0..6 {
        deep.push(format!("a_rather_long_directory_name_{level}"));
    }
    std::fs::create_dir_all(&deep).unwrap();
    let mut model = Model {
        width: 80,
        height: 24,
        file_picker_mode: FilePickerMode::Open,
        ..Model::default()
    };
    model.file_picker.cwd = deep;
    model.file_picker.open_browser();

    let frame = dexo_tui::render::render_to_string(&model, 80, 24);

    assert!(frame.contains("a_rather_long_directory_name_5"), "{frame}");
    assert!(frame.contains('…'), "{frame}");
}

#[test]
fn page_keys_and_home_end_move_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut model = crowded(dir.path(), 24);
    model.file_picker.open_browser();
    model.file_picker.focus = FilePickerFocus::List;
    let last = model.file_picker.entries.len() - 1;

    press(&mut model, KeyCode::PageDown);
    assert!(model.file_picker.selected > 5, "PageDown did not page");
    press(&mut model, KeyCode::PageUp);
    assert_eq!(model.file_picker.selected, 0);
    press(&mut model, KeyCode::End);
    assert_eq!(model.file_picker.selected, last);
    press(&mut model, KeyCode::Home);
    assert_eq!(model.file_picker.selected, 0);
}

/// A name longer than the field scrolls with the cursor: the end the user is typing
/// stays on screen.
#[test]
fn a_long_name_scrolls_with_the_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let mut model = crowded(dir.path(), 24);
    model.file_picker_mode = FilePickerMode::Save;
    model.file_picker.focus = FilePickerFocus::Name;
    model.file_picker.name.set_text(format!(
        "{}-the-end.sql",
        "monthly_revenue_report_".repeat(5)
    ));

    let frame = dexo_tui::render::render_to_string(&model, 80, 24);

    assert!(frame.contains("the-end.sql"), "{frame}");
}
