use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Paragraph};

use crate::model::Model;

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let body = if model.sql.is_empty() {
        "-- editor".into()
    } else {
        model.sql.clone()
    };
    if area.width < 2 || area.height < 2 {
        frame.render_widget(Paragraph::new(body), area);
        return;
    }
    frame.render_widget(
        Paragraph::new(body).block(Block::bordered().title("SQL")),
        area,
    );
}
