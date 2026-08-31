use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
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

fn inspector_with_ddl(lines: usize) -> Model {
    let mut model = Model::default();
    model.inspector.open = true;
    model.inspector.qualified_name = "public.events".into();
    model.inspector.ddl = Some(
        (0..lines)
            .map(|line| format!("column_{line} text"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    model
}

#[test]
fn clicking_ddl_inspector_tab_selects_the_same_tab_as_keyboard_cycle() {
    let mut model = inspector_with_ddl(1);
    paint(&mut model);

    let (column, row) = model.hits.center(HitTarget::InspectorTab(1));
    update(
        &mut model,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
    );

    let mut expected = inspector_with_ddl(1);
    update(&mut expected, Action::InspectorNextTab);
    assert_eq!(model.inspector.tab, expected.inspector.tab);
    assert_eq!(model.focus, Focus::Inspector);
}

#[test]
fn keyboard_inspector_tab_cycle_resets_scroll_like_mouse_tab_selection() {
    let mut keyboard = inspector_with_ddl(100);
    keyboard.inspector.scroll = 50;

    update(&mut keyboard, Action::InspectorNextTab);

    assert_eq!(keyboard.inspector.scroll, 0);

    let mut mouse_model = inspector_with_ddl(100);
    mouse_model.inspector.scroll = 50;
    paint(&mut mouse_model);
    let (column, row) = mouse_model.hits.center(HitTarget::InspectorTab(1));
    update(
        &mut mouse_model,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
    );

    assert_eq!(keyboard.inspector.tab, mouse_model.inspector.tab);
    assert_eq!(keyboard.inspector.scroll, mouse_model.inspector.scroll);
}

#[test]
fn wheel_over_inspector_scrolls_long_content_without_moving_results() {
    let mut model = inspector_with_ddl(100);
    paint(&mut model);
    let (column, row) = model.hits.center(HitTarget::Inspector);

    update(&mut model, mouse(MouseEventKind::ScrollDown, column, row));

    assert_eq!(model.inspector.scroll, 1);
}

#[test]
fn wheel_over_results_does_not_scroll_inspector() {
    let mut model = inspector_with_ddl(100);
    *model.results = dexo_tui::model::GridModel::sample_rows(4);
    model.results.select_row(0);
    paint(&mut model);
    let (column, row) = model.hits.center(HitTarget::Grid);

    update(&mut model, mouse(MouseEventKind::ScrollDown, column, row));

    assert_eq!(model.inspector.scroll, 0);
    assert_eq!(model.results.selection(), Some((1, 0)));
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
fn dragging_inspector_divider_left_increases_inspector_width() {
    let mut model = Model::default();
    let divider_x = model.width - model.panes.inspector_width;
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
            divider_x - 10,
            divider_y,
        ),
    );

    assert_eq!(model.panes.inspector_width, 38);
}
