use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::mouse::HitTarget;
use dexo_tui::screens::file_picker::{FilePickerFocus, FilePickerMode};
use dexo_tui::{Effect, Focus, Model, update};

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn saving(dir: &std::path::Path, name: &str) -> Model {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    model
        .active_document_mut()
        .sql
        .insert(0, "select 42 as answer")
        .unwrap();
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
    );
    assert!(model.file_picker.open, "Ctrl+S did not ask where to save");
    model.file_picker.cwd = dir.to_path_buf();
    model.file_picker.refresh();
    model.file_picker.focus = FilePickerFocus::Name;
    model.file_picker.name.set_text(name);
    model
}

fn saves(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, Effect::SaveDocument(_)))
}

/// The Save picker wrote straight over whatever file the name pointed at: a database
/// sitting in the folder was replaced by 62 bytes of SQL.
#[test]
fn saving_over_a_file_asks_first_and_cancel_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("victim.txt"), "precious").unwrap();
    let mut model = saving(dir.path(), "victim.txt");

    let effects = press(&mut model, KeyCode::Enter);

    assert!(!saves(&effects), "the file was written without asking");
    assert!(model.file_picker.open);
    let frame = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(frame.contains("victim.txt already exists."), "{frame}");
    assert!(frame.contains("[Replace]"), "{frame}");

    // Cancel is where the question starts: Enter on it keeps the file.
    let effects = press(&mut model, KeyCode::Enter);
    assert!(!saves(&effects));
    assert!(model.file_picker.confirm.is_none());
    assert!(model.file_picker.open, "Cancel left the picker too");
    assert_eq!(model.active_document().path, None);
}

#[test]
fn escape_on_the_question_goes_back_to_the_picker() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("victim.txt"), "precious").unwrap();
    let mut model = saving(dir.path(), "victim.txt");
    press(&mut model, KeyCode::Enter);

    press(&mut model, KeyCode::Esc);

    assert!(model.file_picker.confirm.is_none());
    assert!(model.file_picker.open);
}

#[test]
fn replace_writes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("victim.txt"), "precious").unwrap();
    let mut model = saving(dir.path(), "victim.txt");
    press(&mut model, KeyCode::Enter);

    press(&mut model, KeyCode::Left);
    let effects = press(&mut model, KeyCode::Enter);

    assert!(saves(&effects), "{effects:?}");
    assert!(!model.file_picker.open);
    assert_eq!(model.active_document().title, "victim.txt");
}

#[test]
fn a_new_name_saves_without_asking() {
    let dir = tempfile::tempdir().unwrap();
    let mut model = saving(dir.path(), "fresh.sql");

    let effects = press(&mut model, KeyCode::Enter);

    assert!(saves(&effects));
    assert!(model.file_picker.confirm.is_none());
}

/// Enter on a highlighted file in the Save picker used to be the shortest way to lose
/// it: the row filled the name and wrote at once.
#[test]
fn enter_on_a_listed_file_asks_before_replacing_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("base.db"), "database").unwrap();
    let mut model = saving(dir.path(), "");
    model.file_picker.focus = FilePickerFocus::List;
    let index = model
        .file_picker
        .entries
        .iter()
        .position(|entry| entry.name == "base.db")
        .unwrap();
    model.file_picker.selected = index;

    let effects = press(&mut model, KeyCode::Enter);

    assert!(!saves(&effects));
    assert!(model.file_picker.confirm.is_some());
}

#[test]
fn the_mouse_answers_the_question() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("victim.txt"), "precious").unwrap();
    let mut model = saving(dir.path(), "victim.txt");
    press(&mut model, KeyCode::Enter);
    let mut hits = dexo_tui::mouse::HitMap::default();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;

    let (x, y) = model.hits.center(HitTarget::FooterSubmit);
    let effects = update(
        &mut model,
        Action::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }),
    );

    assert!(saves(&effects), "{effects:?}");
}

/// Exporting is a write too; importing is a read and never asks.
#[test]
fn the_transfer_picker_asks_only_when_it_writes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("out.csv"), "a,b").unwrap();
    for (mode, asks) in [
        (dexo_tui::screens::transfer::TransferMode::Export, true),
        (dexo_tui::screens::transfer::TransferMode::Backup, true),
        (dexo_tui::screens::transfer::TransferMode::Import, false),
    ] {
        let mut model = Model::default();
        model.transfer.open = true;
        model.transfer.mode = mode;
        model.file_picker_mode = FilePickerMode::Transfer;
        model.file_picker.open_browser();
        model.file_picker.cwd = dir.path().to_path_buf();
        model.file_picker.name.set_text("out.csv");
        model.file_picker.focus = FilePickerFocus::Name;

        press(&mut model, KeyCode::Enter);

        assert_eq!(model.file_picker.confirm.is_some(), asks, "{mode:?}");
    }
}
