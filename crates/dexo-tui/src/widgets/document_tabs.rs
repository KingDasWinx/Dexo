use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};

pub fn labels(model: &Model) -> Vec<String> {
    model
        .documents
        .iter()
        .map(|document| {
            let dirty = if document.is_dirty() { "*" } else { "" };
            format!(" {}{dirty} ", document.title)
        })
        .collect()
}

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut x = area.x;
    let end = area.x.saturating_add(area.width);
    let mut spans = Vec::new();
    for (index, label) in labels(model).into_iter().enumerate() {
        let width = UnicodeWidthStr::width(label.as_str()) as u16;
        if width == 0 || x.saturating_add(width) > end {
            break;
        }
        let style = if index == model.active_document {
            model.theme.active_row(model.capabilities)
        } else {
            model.theme.pane_title(false, model.capabilities)
        };
        spans.push(Span::styled(label, style));
        hits.register(
            HitTarget::DocumentTab(index),
            Rect::new(x, area.y, width, 1),
        );
        x = x.saturating_add(width);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use dexo_sql::SqlDocument;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    use super::labels;
    use crate::model::{EditorDocument, Model};
    use crate::mouse::{HitMap, HitTarget};

    #[test]
    fn document_tab_labels_mark_dirty_and_active() {
        let mut model = Model::default();
        model.documents = vec![
            EditorDocument::new_unique("console.sql", None, None),
            EditorDocument::new_unique("q2.sql", None, None),
        ];
        model.documents[1].sql = SqlDocument::new("select 1");
        model.documents[1].sql.insert(0, " ").unwrap();
        model.active_document = 1;

        let labels = labels(&model);

        assert_eq!(labels, vec![" console.sql ", " q2.sql* "]);
    }

    #[test]
    fn document_tabs_register_click_targets() {
        let mut model = Model::default();
        model
            .documents
            .push(EditorDocument::new_unique("q2.sql", None, None));
        let mut terminal = Terminal::new(TestBackend::new(30, 1)).unwrap();
        let mut hits = HitMap::default();

        terminal
            .draw(|frame| super::render(frame, Rect::new(0, 0, 30, 1), &model, &mut hits))
            .unwrap();

        assert_eq!(hits.at(1, 0), Some(HitTarget::DocumentTab(0)));
        assert_eq!(hits.at(14, 0), Some(HitTarget::DocumentTab(1)));
    }
}
