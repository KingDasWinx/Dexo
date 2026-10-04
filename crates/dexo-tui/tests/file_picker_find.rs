//! Open file finds as it is typed into: SQL files in the folder and below, a path typed
//! goes there, and Backspace with nothing typed goes up a folder. It listed every file
//! with a name field nobody knew what to do with, and its keys in a row of its own.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::{Action, Effect};
use dexo_tui::screens::file_picker::FilePickerMode;
use dexo_tui::{Model, update};

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn typed(model: &mut Model, text: &str) {
    for ch in text.chars() {
        press(model, KeyCode::Char(ch));
    }
}

/// A project: two SQL files and a note at the top, a report two folders down.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("sql/reports")).unwrap();
    std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    std::fs::write(dir.path().join("orders.sql"), "select 1").unwrap();
    std::fs::write(dir.path().join("users.sql"), "select 2").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
    std::fs::write(dir.path().join("sql/reports/monthly.sql"), "select 3").unwrap();
    std::fs::write(dir.path().join("node_modules/pkg/monthly.sql"), "x").unwrap();
    dir
}

fn open_in(dir: &std::path::Path, mode: FilePickerMode) -> Model {
    let mut model = Model::default();
    model.apply_size(120, 40);
    model.file_picker.cwd = dir.to_path_buf();
    model.file_picker.finding = mode == FilePickerMode::Open;
    model.file_picker_mode = mode;
    model.file_picker.open_browser();
    model
}

fn names(model: &Model) -> Vec<String> {
    model
        .file_picker
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect()
}

#[test]
fn open_lists_folders_and_sql_files_only() {
    let dir = project();
    let model = open_in(dir.path(), FilePickerMode::Open);

    let listed = names(&model);

    assert!(listed.contains(&"orders.sql".to_string()), "{listed:?}");
    assert!(listed.contains(&"sql".to_string()), "{listed:?}");
    assert!(!listed.contains(&"notes.txt".to_string()), "{listed:?}");
}

#[test]
fn typing_finds_below_the_folder_and_enter_opens_it() {
    let dir = project();
    let mut model = open_in(dir.path(), FilePickerMode::Open);

    typed(&mut model, "month");

    let listed = names(&model);
    // As the platform writes a path: `sql\reports\monthly.sql` on Windows.
    let found = std::path::Path::new("sql")
        .join("reports")
        .join("monthly.sql")
        .display()
        .to_string();
    assert!(listed.contains(&found), "{listed:?}");
    assert!(
        !listed.iter().any(|name| name.contains("node_modules")),
        "a package's folder is not looked into: {listed:?}"
    );
    assert!(!listed.contains(&"orders.sql".to_string()));
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadDocument(request)
                if request.path.ends_with("sql/reports/monthly.sql")
        )),
        "{effects:?}"
    );
}

#[test]
fn backspace_unfinds_then_goes_up_a_folder() {
    let dir = project();
    let mut model = open_in(&dir.path().join("sql"), FilePickerMode::Open);
    typed(&mut model, "x");
    assert_eq!(model.file_picker.name.as_str(), "x");

    press(&mut model, KeyCode::Backspace);
    assert_eq!(model.file_picker.cwd, dir.path().join("sql"));
    press(&mut model, KeyCode::Backspace);

    assert_eq!(model.file_picker.cwd, dir.path());
}

#[test]
fn a_path_typed_is_gone_to() {
    let dir = project();
    let mut model = open_in(dir.path(), FilePickerMode::Open);

    typed(&mut model, "sql/reports/");
    press(&mut model, KeyCode::Enter);

    assert_eq!(model.file_picker.cwd, dir.path().join("sql/reports"));
    assert!(model.file_picker.name.is_empty());
}

/// Saving, a letter typed on the list starts the file's name: it jumped to the first
/// file starting with it, and `h` hid the hidden files.
#[test]
fn saving_types_the_name_from_the_list() {
    let dir = project();
    let mut model = open_in(dir.path(), FilePickerMode::Save);

    typed(&mut model, "hello");

    assert_eq!(model.file_picker.name.as_str(), "hello");
    assert!(!model.file_picker.show_hidden);
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)),
    );
    assert!(
        model.file_picker.show_hidden,
        "Alt+H shows the hidden files"
    );
}

/// Its keys are on the status line, not in a row of the dialog.
#[test]
fn the_keys_are_on_the_status_line() {
    let dir = project();
    let mut model = open_in(dir.path(), FilePickerMode::Open);
    model.file_picker.open = true;

    let frame = dexo_tui::render::render_to_string(&model, 100, 30);

    let last = frame.lines().last().unwrap_or_default().to_string();
    assert!(last.contains("type to find"), "{frame}");
    assert!(frame.contains("Find:"), "{frame}");
    assert!(!frame.contains("PgUp/PgDn scroll"), "{frame}");
}
