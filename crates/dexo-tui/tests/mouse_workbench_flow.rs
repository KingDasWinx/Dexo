use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::{Action, Focus, Model, update};

fn paint(model: &mut Model) {
    let width = model.width.max(80);
    let height = model.height.max(24);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Action {
    Action::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn dragging_explorer_divider_resizes_pane_and_releases_capture() {
    let mut model = Model::default();
    let divider_x = model.panes.explorer_width;
    let divider_y = 10;
    paint(&mut model);

    update(
        &mut model,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            divider_x,
            divider_y,
        ),
    );
    update(
        &mut model,
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            divider_x + 10,
            divider_y,
        ),
    );
    update(
        &mut model,
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            divider_x + 10,
            divider_y,
        ),
    );

    assert_eq!(model.panes.explorer_width, 38);
    assert!(model.layout_dirty);
    assert_eq!(model.drag, None);
}

#[test]
fn dragging_results_divider_up_increases_results_height() {
    let mut model = Model::default();
    let divider_x = 80;
    let divider_y = 37;
    paint(&mut model);

    update(
        &mut model,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            divider_x,
            divider_y,
        ),
    );
    update(
        &mut model,
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            divider_x,
            divider_y - 5,
        ),
    );

    assert_eq!(model.panes.results_height, 17);
}

#[test]
fn dragging_in_editor_selects_the_text_between_mouse_positions() {
    let mut model = Model::default();
    model.set_sql(
        (0..40)
            .map(|_| "0123456789".repeat(20))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    paint(&mut model);
    let (column, row) = model.hits.center(HitTarget::Editor);

    update(
        &mut model,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
    );
    update(
        &mut model,
        mouse(MouseEventKind::Drag(MouseButton::Left), column + 4, row),
    );

    let selection = model.active_document().selection().unwrap();
    assert_eq!(selection.end - selection.start, 4);
    update(
        &mut model,
        mouse(MouseEventKind::Up(MouseButton::Left), column + 4, row),
    );
    assert_eq!(model.drag, None);
}

#[test]
fn clicking_in_editor_moves_caret_and_clears_existing_selection() {
    let mut model = Model::default();
    model.set_sql(
        (0..40)
            .map(|_| "0123456789".repeat(20))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    model.active_document_mut().anchor = Some(0);
    let _ = model.active_document_mut().sql.set_cursor(6);
    paint(&mut model);
    let (column, row) = model.hits.center(HitTarget::Editor);

    update(
        &mut model,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
    );

    assert!(model.active_document().selection().is_none());
    assert_ne!(model.active_document().cursor(), 6);
}

#[test]
fn clicking_each_workbench_pane_matches_its_alt_focus_shortcut() {
    let cases = [
        (HitTarget::Explorer, KeyCode::Char('1'), Focus::Explorer),
        (HitTarget::Editor, KeyCode::Char('2'), Focus::Editor),
        (HitTarget::Grid, KeyCode::Char('3'), Focus::Results),
    ];

    for (target, key, focus) in cases {
        let mut mouse_model = Model {
            focus: Focus::Editor,
            ..Model::default()
        };
        paint(&mut mouse_model);
        let (column, row) = mouse_model.hits.center(target);
        assert_ne!((column, row), (0, 0), "{target:?} must be painted");
        update(
            &mut mouse_model,
            mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        );

        let mut keyboard_model = Model {
            focus: Focus::Editor,
            ..Model::default()
        };
        update(
            &mut keyboard_model,
            Action::Key(KeyEvent::new(key, KeyModifiers::ALT)),
        );

        assert_eq!(mouse_model.focus, focus);
        assert_eq!(mouse_model.focus, keyboard_model.focus);
        assert_eq!(mouse_model.panes, keyboard_model.panes);
    }
}

/// The selector's rects are derived from the title string rather than from a laid-out
/// row, so the arithmetic has to be pinned: a click must land on the label under it.
#[test]
fn clicking_the_results_view_selector_lands_on_its_label() {
    use dexo_tui::model::ResultsView;

    let mut model = Model::default();
    paint(&mut model);

    let (column, row) = model.hits.center(HitTarget::ResultsView(1));
    assert_ne!((column, row), (0, 0), "Explain label must be painted");
    let view = dexo_tui::render::render_to_string(&model, model.width, model.height);
    let line = view.lines().nth(row as usize).expect("selector row");
    // the hit rect spans the label, so the click lands mid-word by design
    let window: String = line
        .chars()
        .skip(column.saturating_sub(5) as usize)
        .take(13)
        .collect();
    assert!(
        window.contains("Explain"),
        "click at {column} lands near {window:?}, not on the Explain label"
    );

    update(
        &mut model,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
    );
    assert_eq!(model.results.view, ResultsView::Explain);

    let (column, row) = model.hits.center(HitTarget::ResultsView(0));
    update(
        &mut model,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
    );
    assert_eq!(model.results.view, ResultsView::Grid);
}

#[test]
fn clicking_workbench_tabs_matches_ctrl_number_shortcuts() {
    for index in 0..2 {
        let mut mouse_model = Model::default();
        paint(&mut mouse_model);
        let (column, row) = mouse_model.hits.center(HitTarget::WorkbenchTab(index));
        assert_ne!((column, row), (0, 0), "tab {index} must be painted");
        update(
            &mut mouse_model,
            mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        );

        let mut keyboard_model = Model::default();
        update(
            &mut keyboard_model,
            Action::Key(KeyEvent::new(
                KeyCode::Char(char::from(b'1' + index as u8)),
                KeyModifiers::CONTROL,
            )),
        );

        assert_eq!(mouse_model.tabs.active, keyboard_model.tabs.active);
    }
}

#[test]
fn dragging_explorer_divider_clamps_to_the_layout_limits() {
    let mut model = Model::default();
    let divider_x = model.panes.explorer_width;
    paint(&mut model);

    update(
        &mut model,
        mouse(MouseEventKind::Down(MouseButton::Left), divider_x, 10),
    );
    update(
        &mut model,
        mouse(MouseEventKind::Drag(MouseButton::Left), u16::MAX, 10),
    );
    update(
        &mut model,
        mouse(MouseEventKind::Up(MouseButton::Left), u16::MAX, 10),
    );

    assert_eq!(model.panes.explorer_width, model.width / 2);
    assert_eq!(model.drag, None);
}
