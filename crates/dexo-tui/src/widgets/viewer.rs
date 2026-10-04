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

/// The value as the modal's body: the text itself, every line of it.
pub fn describe(view: &ValueView) -> String {
    match view {
        ValueView::Null => "NULL".into(),
        ValueView::Text(text) | ValueView::Xml(text) | ValueView::Array(text) => text.clone(),
        ValueView::JsonPretty(text) => text.clone(),
        ValueView::Hex(text) => format!("\\x{text}"),
        ValueView::Image {
            mime,
            width: Some(width),
            height: Some(height),
        } => format!("An image: {mime}, {width} x {height} pixels"),
        ValueView::Image { mime, .. } => format!("An image: {mime}"),
        ValueView::Truncated { loaded, total } => format!(
            "Only {} of this {} value are loaded here. Export the rows to read all of it.",
            size(*loaded),
            size(*total)
        ),
        ValueView::Unloaded { total, .. } => {
            format!("This {} value is not loaded yet.", size(*total))
        }
    }
}

/// What the modal's title says about the value: its kind and how big it is.
pub fn summary(view: &ValueView) -> String {
    let characters = |text: &str| {
        let count = text.chars().count();
        format!("{count} character{}", if count == 1 { "" } else { "s" })
    };
    match view {
        ValueView::Null => "NULL".into(),
        ValueView::Text(text) => format!("text, {}", characters(text)),
        ValueView::Xml(text) => format!("XML, {}", characters(text)),
        ValueView::Array(text) => format!("array, {}", characters(text)),
        ValueView::JsonPretty(_) => "JSON".into(),
        ValueView::Hex(text) => format!("binary, {}", size(text.len() as u64 / 2)),
        ValueView::Image { mime, .. } => (*mime).to_string(),
        ValueView::Truncated { total, .. } | ValueView::Unloaded { total, .. } => {
            format!("{}, not fully loaded", size(*total))
        }
    }
}

/// `512 bytes`, `64 KiB`, `100 MiB`.
fn size(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KiB", "MiB", "GiB"];
    if bytes < 1024 {
        return format!("{bytes} byte{}", if bytes == 1 { "" } else { "s" });
    }
    let mut value = bytes as f64;
    let mut unit = "bytes";
    for next in UNITS {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    format!("{value:.1} {unit}").replace(".0 ", " ")
}

#[cfg(test)]
mod tests {
    use super::{describe, summary};
    use dexo_app::data::ValueView;

    #[test]
    fn truncated_100mb_stays_bounded() {
        let view = ValueView::Truncated {
            loaded: 16,
            total: 100 * 1024 * 1024,
        };
        let text = describe(&view);
        assert!(text.contains("100 MiB"), "{text}");
        assert!(text.contains("16 bytes"), "{text}");
        assert!(!text.contains("loaded:") && !text.contains('{'), "{text}");
        assert!(text.len() < 128);
    }

    #[test]
    fn the_title_says_what_the_value_is() {
        assert_eq!(
            summary(&ValueView::Text("é".repeat(400))),
            "text, 400 characters"
        );
        assert_eq!(summary(&ValueView::Text("x".into())), "text, 1 character");
        assert_eq!(summary(&ValueView::Hex("00ff".into())), "binary, 2 bytes");
    }
}
