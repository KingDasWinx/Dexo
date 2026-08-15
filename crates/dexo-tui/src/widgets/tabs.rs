use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Tabs;

use crate::model::Model;

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
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
