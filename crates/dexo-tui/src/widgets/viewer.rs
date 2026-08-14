use dexo_app::data::ValueView;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Paragraph};

use crate::model::Model;

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let body = match &model.data.viewer {
        None => "No value".into(),
        Some(view) => describe(view),
    };
    if area.width < 2 || area.height < 2 {
        frame.render_widget(Paragraph::new(body), area);
        return;
    }
    frame.render_widget(
        Paragraph::new(body).block(Block::bordered().title("Value")),
        area,
    );
}

pub fn describe(view: &ValueView) -> String {
    match view {
        ValueView::Null => "NULL".into(),
        ValueView::Text(text) | ValueView::Xml(text) | ValueView::Array(text) => text.clone(),
        ValueView::JsonPretty(text) => text.clone(),
        ValueView::Hex(text) => format!("hex:{text}"),
        ValueView::Image { mime, .. } => format!("image:{mime}"),
        ValueView::Truncated { loaded, total } => {
            format!("Truncated {{ loaded:{loaded}, total:{total} }}")
        }
        ValueView::Unloaded { total, .. } => format!("unloaded total:{total}"),
    }
}

#[cfg(test)]
mod tests {
    use super::describe;
    use dexo_app::data::ValueView;

    #[test]
    fn truncated_100mb_stays_bounded() {
        let view = ValueView::Truncated {
            loaded: 16,
            total: 100 * 1024 * 1024,
        };
        let text = describe(&view);
        assert!(text.contains("Truncated"));
        assert!(text.contains("104857600") || text.contains("total:"));
        assert!(text.len() < 128);
    }
}
