use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Tabs;

use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let mut x = area.x;
    for (index, title) in model.tabs.titles.iter().enumerate() {
        let label = if index == 0 && model.active_document().is_dirty() {
            format!(" {title}* ")
        } else {
            format!(" {title} ")
        };
        let width =
            (label.chars().count() as u16).min(area.width.saturating_sub(x.saturating_sub(area.x)));
        if width == 0 {
            break;
        }
        hits.register(
            HitTarget::WorkbenchTab(index),
            Rect::new(x, area.y, width, area.height.min(1)),
        );
        x = x.saturating_add(width);
        if x >= area.x.saturating_add(area.width) {
            break;
        }
    }
    let tabs = Tabs::new(model.tabs.titles.iter().enumerate().map(|(index, title)| {
        if index == 0 && model.active_document().is_dirty() {
            format!("{title}*")
        } else {
            title.clone()
        }
    }))
    .select(model.tabs.active);
    frame.render_widget(tabs, area);
}
