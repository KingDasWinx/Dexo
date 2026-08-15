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
        Some(Action::Focus(dexo_tui::action::FocusTarget::Results))
    );
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
