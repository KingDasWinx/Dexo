use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::mouse::{HitButton, HitMap, HitTarget};
use dexo_tui::terminal::{RecordingTerminal, TerminalControl, TerminalGuard};
use dexo_tui::update;
use ratatui::layout::Rect;

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

fn mouse(
    kind: crossterm::event::MouseEventKind,
    column: u16,
    row: u16,
    modifiers: crossterm::event::KeyModifiers,
) -> Action {
    Action::Mouse(crossterm::event::MouseEvent {
        kind,
        column,
        row,
        modifiers,
    })
}

fn click_target(model: &mut Model, target: HitTarget) {
    let (x, y) = model.hits.center(target);
    update(
        model,
        mouse(
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            x,
            y,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
}

#[test]
fn click_on_second_result_tab_selects_that_tab() {
    let mut map = HitMap::default();
    map.register(HitTarget::ResultTab(0), Rect::new(0, 0, 10, 1));
    map.register(HitTarget::ResultTab(1), Rect::new(10, 0, 10, 1));
    map.register(HitTarget::ResultTab(2), Rect::new(20, 0, 10, 1));
    let (x, y) = map.center(HitTarget::ResultTab(2));
    let mut model = Model::default();
    model.results.tabs = vec![
        dexo_tui::model::ResultTab::new(
            dexo_tui::model::ResultKey {
                operation: dexo_tui::runtime::OperationKey::new(
                    dexo_tui::runtime::OperationId::new(),
                    "",
                    "scratch",
                    1,
                ),
                index: 0,
            },
            "r0",
        ),
        dexo_tui::model::ResultTab::new(
            dexo_tui::model::ResultKey {
                operation: dexo_tui::runtime::OperationKey::new(
                    dexo_tui::runtime::OperationId::new(),
                    "",
                    "scratch",
                    1,
                ),
                index: 1,
            },
            "r1",
        ),
        dexo_tui::model::ResultTab::new(
            dexo_tui::model::ResultKey {
                operation: dexo_tui::runtime::OperationKey::new(
                    dexo_tui::runtime::OperationId::new(),
                    "",
                    "scratch",
                    1,
                ),
                index: 2,
            },
            "r2",
        ),
    ];
    model.hits = map;
    update(
        &mut model,
        mouse(
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            x,
            y,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert_eq!(model.results.active, 2);
    assert_eq!(model.focus, dexo_tui::model::Focus::Results);
}

#[test]
fn clicking_explorer_pane_uses_focus_action_path() {
    use dexo_tui::action::FocusTarget;
    use dexo_tui::model::Focus;

    let mut model = Model::default();
    model.focus = Focus::Editor;
    paint(&mut model);
    let (x, y) = model.hits.center(HitTarget::Explorer);
    assert_ne!((x, y), (0, 0));

    let mut expected = model.clone();
    expected.panes.explorer_visible = false;
    update(&mut expected, Action::Focus(FocusTarget::Explorer));

    model.panes.explorer_visible = false;
    update(
        &mut model,
        mouse(
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            x,
            y,
            crossterm::event::KeyModifiers::NONE,
        ),
    );

    assert_eq!(model.focus, expected.focus);
    assert_eq!(model.panes, expected.panes);
}

#[test]
fn keyboard_opens_overlays_without_mouse() {
    let mut model = Model {
        mouse: false,
        ..Model::default()
    };
    update(&mut model, Action::OpenSettings);
    assert!(model.settings.open);
    update(&mut model, Action::OpenAdmin);
    assert!(model.admin.open);
    update(&mut model, Action::OpenRecovery);
    assert!(model.recovery.open);
}

#[test]
fn shift_click_extends_results_selection() {
    use dexo_tui::model::{GridModel, GridSelection};

    let mut model = Model::default();
    *model.results = GridModel::sample_rows(8);
    model.results.select_cell(1, 0);
    model
        .hits
        .register(HitTarget::GridRow(4), Rect::new(0, 10, 20, 1));
    update(
        &mut model,
        mouse(
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            2,
            10,
            crossterm::event::KeyModifiers::SHIFT,
        ),
    );
    assert_eq!(model.focus, dexo_tui::model::Focus::Results);
    assert!(matches!(
        model.results.kind,
        GridSelection::Range {
            start: (1, _),
            end: (4, _)
        }
    ));
}

#[test]
fn mouse_capture_trait_records_on_and_off() {
    let backend = RecordingTerminal::default();
    backend.mouse_capture(true).unwrap();
    backend.mouse_capture(false).unwrap();
    assert_eq!(backend.calls(), vec!["mouse_on", "mouse_off"]);
    let backend = RecordingTerminal::default();
    {
        let mut guard = TerminalGuard::start(backend.clone()).unwrap();
        guard.set_mouse(true).unwrap();
        guard.set_mouse(false).unwrap();
    }
    assert_eq!(
        backend.calls(),
        vec![
            "enter",
            "raw_on",
            "mouse_on",
            "mouse_off",
            "raw_off",
            "leave",
            "cursor_show"
        ]
    );
}

#[test]
fn click_workbench_tab_switches_tab() {
    let mut model = Model::default();
    paint(&mut model);
    click_target(&mut model, HitTarget::WorkbenchTab(1));
    assert_eq!(model.tabs.active, 1);
    click_target(&mut model, HitTarget::WorkbenchTab(2));
    assert_eq!(model.tabs.active, 2);
}

#[test]
fn click_palette_item_runs_command() {
    let mut model = Model::default();
    update(&mut model, Action::OpenPalette);
    update(&mut model, Action::PaletteQuery("settings.open".into()));
    paint(&mut model);
    click_target(&mut model, HitTarget::ListRow(0));
    assert!(model.settings.open);
    assert!(!model.palette.open);
}

#[test]
fn connection_advanced_options_expand_with_the_mouse() {
    let mut model = Model::default();
    update(&mut model, Action::OpenConnectionForm);
    assert!(!model.connection_form.advanced);
    assert!(!model.connection_form.lines().join("\n").contains("tls_mode:"));

    paint(&mut model);
    click_target(
        &mut model,
        HitTarget::Button(HitButton::ToggleAdvanced),
    );
    assert!(model.connection_form.advanced);
    assert!(model.connection_form.lines().join("\n").contains("tls_mode:"));

    paint(&mut model);
    let environment = model
        .connection_form
        .fields
        .iter()
        .position(|field| field.label == "environment")
        .unwrap();
    click_target(&mut model, HitTarget::FormField(environment));
    assert_eq!(model.connection_form.focus, environment);
}

#[test]
fn overlay_click_does_not_fall_through_to_explorer() {
    let mut model = Model::default();
    model.focus = dexo_tui::model::Focus::Editor;
    update(&mut model, Action::OpenSettings);
    paint(&mut model);
    let (x, y) = model.hits.center(HitTarget::Explorer);
    assert_eq!(
        (x, y),
        (0, 0),
        "workbench hits must not be registered under an overlay"
    );
    update(
        &mut model,
        mouse(
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            2,
            8,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert_eq!(model.focus, dexo_tui::model::Focus::Editor);
    assert!(model.settings.open);
}

#[test]
fn scroll_wheel_moves_palette_selection() {
    let mut model = Model::default();
    update(&mut model, Action::OpenPalette);
    paint(&mut model);
    let before = model.palette.selected;
    update(
        &mut model,
        mouse(
            crossterm::event::MouseEventKind::ScrollDown,
            40,
            12,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert!(model.palette.selected > before);
}

#[test]
fn mouse_ignored_when_disabled() {
    let mut model = Model {
        mouse: false,
        ..Model::default()
    };
    paint(&mut model);
    let active = model.tabs.active;
    click_target(&mut model, HitTarget::WorkbenchTab(3));
    assert_eq!(model.tabs.active, active);
}

#[test]
fn ctrl_click_toggles_picked_row() {
    use dexo_tui::model::GridModel;

    let mut model = Model::default();
    *model.results = GridModel::sample_rows(6);
    model.results.select_cell(0, 0);
    model
        .hits
        .register(HitTarget::GridRow(2), Rect::new(0, 12, 20, 1));
    update(
        &mut model,
        mouse(
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            2,
            12,
            crossterm::event::KeyModifiers::CONTROL,
        ),
    );
    assert!(model.results.picked_rows.contains(&2));
}
