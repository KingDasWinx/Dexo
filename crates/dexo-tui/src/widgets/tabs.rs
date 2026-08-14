use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Tabs;

use crate::model::Model;

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let tabs = Tabs::new(model.tabs.titles.iter().map(String::as_str)).select(model.tabs.active);
    frame.render_widget(tabs, area);
}
