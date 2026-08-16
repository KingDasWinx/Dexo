use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::mouse::{HitMap, HitTarget, mouse_action};
use dexo_tui::update;
use ratatui::layout::Rect;

#[test]
fn click_on_second_result_tab_selects_that_tab() {
    let mut map = HitMap::default();
    map.register(HitTarget::ResultTab(0), Rect::new(0, 0, 10, 1));
    map.register(HitTarget::ResultTab(1), Rect::new(10, 0, 10, 1));
    map.register(HitTarget::ResultTab(2), Rect::new(20, 0, 10, 1));
    let (x, y) = map.center(HitTarget::ResultTab(2));
    assert_eq!(
        mouse_action(x, y, &map),
        Some(Action::SelectResultTab { index: 2 })
    );
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
        Action::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: x,
            row: y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }),
    );
    assert_eq!(model.results.active, 2);
    assert_eq!(model.focus, dexo_tui::model::Focus::Results);
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
    use ratatui::layout::Rect;

    let mut model = Model::default();
    *model.results = GridModel::sample_rows(8);
    model.results.select_cell(1, 0);
    model
        .hits
        .register(HitTarget::GridRow(4), Rect::new(0, 10, 20, 1));
    update(
        &mut model,
        Action::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 10,
            modifiers: crossterm::event::KeyModifiers::SHIFT,
        }),
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
