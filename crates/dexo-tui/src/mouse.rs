use crate::action::Action;
use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HitTarget {
    ResultTab(usize),
    Explorer,
    Editor,
    Grid,
    ModalButton,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HitMap {
    targets: Vec<(HitTarget, Rect)>,
}

impl HitMap {
    pub fn register(&mut self, target: HitTarget, rect: Rect) {
        self.targets.push((target, rect));
    }

    pub fn clear(&mut self) {
        self.targets.clear();
    }

    pub fn at(&self, x: u16, y: u16) -> Option<HitTarget> {
        self.targets
            .iter()
            .rev()
            .find(|(_, rect)| {
                x >= rect.x
                    && y >= rect.y
                    && x < rect.x.saturating_add(rect.width)
                    && y < rect.y.saturating_add(rect.height)
            })
            .map(|(target, _)| *target)
    }

    pub fn center(&self, target: HitTarget) -> (u16, u16) {
        self.targets
            .iter()
            .find(|(candidate, _)| *candidate == target)
            .map(|(_, rect)| (rect.x + rect.width / 2, rect.y + rect.height / 2))
            .unwrap_or((0, 0))
    }
}

pub fn mouse_action(x: u16, y: u16, map: &HitMap) -> Option<Action> {
    match map.at(x, y)? {
        HitTarget::ResultTab(index) => Some(Action::SelectResultTab { index }),
        HitTarget::Grid => Some(Action::Focus(crate::action::FocusTarget::Results)),
        HitTarget::Explorer => Some(Action::Focus(crate::action::FocusTarget::Explorer)),
        HitTarget::Editor => Some(Action::Focus(crate::action::FocusTarget::Editor)),
        HitTarget::ModalButton => None,
    }
}
