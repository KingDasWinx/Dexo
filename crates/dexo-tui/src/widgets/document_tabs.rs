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
        let title_width = UnicodeWidthStr::width(label.as_str()) as u16;
        let close_label = "× ";
        let close_width = UnicodeWidthStr::width(close_label) as u16;
        let tab_width = title_width.saturating_add(close_width);
        if title_width == 0 || x.saturating_add(tab_width) > end {
            break;
        }
        let style = if index == model.active_document {
            model.theme.active_row(model.capabilities)
        } else {
            model.theme.pane_title(false, model.capabilities)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::styled(close_label, style));
        hits.register(
            HitTarget::DocumentTab(index),
            Rect::new(x, area.y, title_width, 1),
        );
        hits.register(
            HitTarget::DocumentTabClose(index),
            Rect::new(x.saturating_add(title_width), area.y, close_width, 1),
        );
        x = x.saturating_add(tab_width);
    }
    let new_label = " + ";
    let new_width = UnicodeWidthStr::width(new_label) as u16;
    if x.saturating_add(new_width) <= end {
        spans.push(Span::raw(new_label));
        hits.register(
            HitTarget::DocumentTabNew,
            Rect::new(x, area.y, new_width, 1),
        );
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
        let mut documents = vec![
            EditorDocument::new_unique("console.sql", None, None),
            EditorDocument::new_unique("q2.sql", None, None),
        ];
        documents[1].sql = SqlDocument::new("select 1");
        documents[1].sql.insert(0, " ").unwrap();
        let model = Model {
            documents,
            active_document: 1,
            ..Model::default()
        };

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
        assert_eq!(hits.at(13, 0), Some(HitTarget::DocumentTabClose(0)));
        assert_eq!(hits.at(16, 0), Some(HitTarget::DocumentTab(1)));
        assert_eq!(hits.at(23, 0), Some(HitTarget::DocumentTabClose(1)));
        assert_eq!(hits.at(25, 0), Some(HitTarget::DocumentTabNew));
    }
}
