use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::{Action, Focus, Model, update};

fn resize(model: &mut Model, width: u16, height: u16) {
    update(model, Action::Resize { width, height });
}

fn draw(model: &mut Model) -> String {
    let (width, height) = (model.width, model.height);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
    dexo_tui::render::render_to_string(model, width, height)
}

fn click(model: &mut Model, target: HitTarget) {
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

/// A pass through a small terminal -- a split, a narrow window, tmux at 80x24 which has
/// 23 rows -- hid the explorer and the results for good, and saved that.
#[test]
fn a_small_terminal_does_not_rewrite_the_layout() {
    let mut model = Model::default();
    resize(&mut model, 120, 36);
    let before = model.workbench_layout();

    resize(&mut model, 40, 12);
    resize(&mut model, 60, 20);
    resize(&mut model, 79, 24);
    resize(&mut model, 120, 36);

    let after = model.workbench_layout();
    assert_eq!(after.explorer_visible, before.explorer_visible);
    assert_eq!(after.results_visible, before.results_visible);
    assert_eq!(after.explorer_width, before.explorer_width);
    assert_eq!(after.results_height, before.results_height);
    let frame = draw(&mut model);
    assert!(frame.contains("Sidebar"), "{frame}");
    assert!(frame.contains("Results"), "{frame}");
}

#[test]
fn the_focus_is_never_on_a_pane_that_is_not_drawn() {
    let mut model = Model::default();
    resize(&mut model, 120, 36);
    update(&mut model, Action::NewDocument);
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    resize(&mut model, 60, 20);
    resize(&mut model, 120, 36);

    let frame = draw(&mut model);

    assert!(frame.contains("Sidebar"), "{frame}");
    assert!(frame.contains("SQL"), "{frame}");
}

/// The panes the user hid stay hidden through the same trip.
#[test]
fn what_the_user_hid_stays_hidden() {
    let mut model = Model::default();
    resize(&mut model, 120, 36);
    update(&mut model, Action::HideExplorer);

    resize(&mut model, 50, 15);
    resize(&mut model, 120, 36);

    assert!(!model.workbench_layout().explorer_visible);
    assert!(model.workbench_layout().results_visible);
}

/// Compact mode draws one pane and only Alt+1..3 changed it: the header row now names the
/// three, and each is a click.
#[test]
fn compact_mode_has_a_clickable_pane_switch() {
    let mut model = Model::default();
    resize(&mut model, 60, 20);
    let frame = draw(&mut model);
    for pane in ["1 Sidebar", "2 SQL", "3 Results"] {
        assert!(frame.contains(pane), "{pane}:\n{frame}");
    }

    click(&mut model, HitTarget::Grid);
    assert_eq!(model.focus, Focus::Results);
    let frame = draw(&mut model);
    assert!(frame.contains("[3 Results]"), "{frame}");

    click(&mut model, HitTarget::Explorer);
    assert_eq!(model.focus, Focus::Explorer);
}

#[test]
fn compact_status_names_the_keys_once_and_in_one_spelling() {
    let mut model = Model::default();
    resize(&mut model, 60, 20);
    model.focus = Focus::Editor;

    let frame = draw(&mut model);

    let status = frame.lines().last().unwrap_or_default();
    assert!(status.contains("Ctrl+P  F1"), "{status}");
    assert!(!status.contains("ctrl+p"), "{status}");
    assert_eq!(status.matches("Ctrl+P").count(), 1, "{status}");
}
