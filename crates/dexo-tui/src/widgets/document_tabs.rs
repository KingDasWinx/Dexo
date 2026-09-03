use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::model::{DocumentTabFocus, Model, truncate_cell};
use crate::mouse::{HitMap, HitTarget};

const CLOSE_LABEL: &str = "× ";
const NEW_LABEL: &str = " + ";
const SCROLL_PREV: &str = "‹";
const SCROLL_NEXT: &str = "›";
const MAX_TITLE_WIDTH: usize = 20;
const MIN_TITLE_WIDTH: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TabItem {
    index: usize,
    label: String,
    title_width: u16,
    close_width: u16,
}

impl TabItem {
    fn total_width(&self) -> u16 {
        self.title_width.saturating_add(self.close_width)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Viewport {
    scroll: usize,
    visible: Vec<TabItem>,
    show_prev: bool,
    show_next: bool,
}

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

fn close_width() -> u16 {
    UnicodeWidthStr::width(CLOSE_LABEL) as u16
}

fn new_button_width() -> u16 {
    UnicodeWidthStr::width(NEW_LABEL) as u16
}

fn scroll_indicator_width() -> u16 {
    1
}

fn tab_items(model: &Model, max_title_width: usize) -> Vec<TabItem> {
    model
        .documents
        .iter()
        .enumerate()
        .map(|(index, document)| {
            let dirty = if document.is_dirty() { "*" } else { "" };
            let title = truncate_cell(document.title.as_str(), max_title_width);
            let padded = format!(" {title}{dirty} ");
            let title_width = UnicodeWidthStr::width(padded.as_str()) as u16;
            TabItem {
                index,
                label: padded,
                title_width,
                close_width: close_width(),
            }
        })
        .collect()
}

fn layout_with_pinned_active(
    items: &[TabItem],
    width: u16,
    active: usize,
    scroll_hint: usize,
) -> Option<Viewport> {
    let new_width = new_button_width();
    if width <= new_width || items.is_empty() {
        return None;
    }

    let active = active.min(items.len() - 1);
    let active_item = items[active].clone();

    let try_pack = |show_prev: bool, show_next: bool| -> Option<Viewport> {
        let chrome = new_width
            + u16::from(show_prev) * scroll_indicator_width()
            + u16::from(show_next) * scroll_indicator_width();
        let mut budget = width.saturating_sub(chrome);
        let mut packed = vec![active_item.clone()];
        let mut packed_left = active;
        let mut packed_right = active;
        budget = budget.saturating_sub(active_item.total_width());

        let preferred_left = scroll_hint.min(active);
        for index in (preferred_left..active).rev() {
            let item = &items[index];
            if item.total_width() <= budget {
                packed.insert(0, item.clone());
                budget = budget.saturating_sub(item.total_width());
                packed_left = index;
            } else {
                break;
            }
        }
        for index in active + 1..items.len() {
            let item = &items[index];
            if item.total_width() <= budget {
                packed.push(item.clone());
                budget = budget.saturating_sub(item.total_width());
                packed_right = index;
            } else {
                break;
            }
        }
        for index in (0..packed_left).rev() {
            let item = &items[index];
            if item.total_width() <= budget {
                packed.insert(0, item.clone());
                budget = budget.saturating_sub(item.total_width());
                packed_left = index;
            } else {
                break;
            }
        }

        let content_width: u16 = packed.iter().map(TabItem::total_width).sum();
        if content_width.saturating_add(chrome) > width {
            while packed.len() > 1 {
                if packed[0].index != active {
                    packed.remove(0);
                } else if packed.last().map(|item| item.index) == Some(active) && packed.len() > 1 {
                    packed.pop();
                } else if packed.last().map(|item| item.index) != Some(active) {
                    packed.pop();
                } else {
                    break;
                }
                let content_width: u16 = packed.iter().map(TabItem::total_width).sum();
                if content_width.saturating_add(chrome) <= width {
                    break;
                }
            }
        }

        let content_width: u16 = packed.iter().map(TabItem::total_width).sum();
        if content_width.saturating_add(chrome) > width {
            return None;
        }

        Some(Viewport {
            scroll: packed_left,
            visible: packed,
            show_prev: show_prev && packed_left > 0,
            show_next: show_next && packed_right + 1 < items.len(),
        })
    };

    let show_prev = scroll_hint > 0 || active > 0;
    let show_next = scroll_hint + 1 < items.len() || active + 1 < items.len();
    if let Some(viewport) = try_pack(show_prev, show_next) {
        return Some(viewport);
    }
    if let Some(viewport) = try_pack(false, show_next) {
        return Some(viewport);
    }
    if let Some(viewport) = try_pack(show_prev, false) {
        return Some(viewport);
    }
    try_pack(false, false)
}

fn resolve_layout(model: &Model, width: u16) -> (Vec<TabItem>, Viewport) {
    if model.documents.is_empty() {
        return (
            Vec::new(),
            Viewport {
                scroll: 0,
                visible: Vec::new(),
                show_prev: false,
                show_next: false,
            },
        );
    }

    let active = model.active_document.min(model.documents.len() - 1);
    let scroll_hint = model
        .document_tabs_scroll
        .min(model.documents.len().saturating_sub(1));

    for max_title in (MIN_TITLE_WIDTH..=MAX_TITLE_WIDTH).rev() {
        let items = tab_items(model, max_title);
        let total_width: u16 = items.iter().map(TabItem::total_width).sum();
        if total_width.saturating_add(new_button_width()) <= width {
            return (
                items.clone(),
                Viewport {
                    scroll: 0,
                    visible: items,
                    show_prev: false,
                    show_next: false,
                },
            );
        }
        if let Some(viewport) = layout_with_pinned_active(&items, width, active, scroll_hint) {
            return (items, viewport);
        }
    }

    let max_title = width
        .saturating_sub(new_button_width())
        .saturating_sub(2)
        .saturating_sub(close_width())
        .max(4) as usize;
    let items = tab_items(model, max_title.max(MIN_TITLE_WIDTH));
    let active = active.min(items.len().saturating_sub(1));
    let pinned = items[active].clone();
    (
        items,
        Viewport {
            scroll: active,
            visible: vec![pinned],
            show_prev: active > 0,
            show_next: active + 1 < model.documents.len(),
        },
    )
}

pub fn sync_scroll(model: &mut Model, width: u16) {
    let (_, viewport) = resolve_layout(model, width);
    model.document_tabs_scroll = viewport.scroll;
}

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (_, viewport) = resolve_layout(model, area.width);
    let mut x = area.x;
    let end = area.x.saturating_add(area.width);
    let mut spans = Vec::new();

    if viewport.show_prev {
        spans.push(Span::raw(SCROLL_PREV));
        hits.register(
            HitTarget::DocumentTabScrollPrev,
            Rect::new(x, area.y, scroll_indicator_width(), 1),
        );
        x = x.saturating_add(scroll_indicator_width());
    }

    for item in &viewport.visible {
        let tab_width = item.total_width();
        if x.saturating_add(tab_width) > end {
            break;
        }
        let style = if matches!(model.document_tab_focus, DocumentTabFocus::Document(i) if i == item.index)
        {
            model.theme.active_row(model.capabilities)
        } else if item.index == model.active_document {
            model.theme.pane_title(true, model.capabilities)
        } else if matches!(model.document_tab_focus, DocumentTabFocus::New)
            && item.index == model.active_document
        {
            model.theme.pane_title(true, model.capabilities)
        } else {
            model.theme.pane_title(false, model.capabilities)
        };
        spans.push(Span::styled(item.label.clone(), style));
        spans.push(Span::styled(CLOSE_LABEL, style));
        hits.register(
            HitTarget::DocumentTab(item.index),
            Rect::new(x, area.y, item.title_width, 1),
        );
        hits.register(
            HitTarget::DocumentTabClose(item.index),
            Rect::new(x.saturating_add(item.title_width), area.y, item.close_width, 1),
        );
        x = x.saturating_add(tab_width);
    }

    if viewport.show_next {
        if x < end {
            spans.push(Span::raw(SCROLL_NEXT));
            hits.register(
                HitTarget::DocumentTabScrollNext,
                Rect::new(x, area.y, scroll_indicator_width(), 1),
            );
            x = x.saturating_add(scroll_indicator_width());
        }
    }

    let new_width = new_button_width();
    if x.saturating_add(new_width) <= end {
        let new_style = if model.document_tab_focus == DocumentTabFocus::New {
            model.theme.active_row(model.capabilities)
        } else {
            Style::default()
        };
        spans.push(Span::styled(NEW_LABEL, new_style));
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
    use unicode_width::UnicodeWidthStr;

    use super::{labels, resolve_layout, sync_scroll, tab_items};
    use crate::model::{DocumentTabFocus, EditorDocument, Model};
    use crate::mouse::{HitMap, HitTarget};
    use crate::render::render_to_string;

    fn many_documents(count: usize) -> Model {
        let mut model = Model::default();
        model.documents.clear();
        for index in 0..count {
            model.documents.push(EditorDocument::new_unique(
                format!("query-{index}.sql"),
                None,
                None,
            ));
        }
        model.active_document = count - 1;
        model.document_tab_focus = DocumentTabFocus::Document(count - 1);
        model
    }

    fn active_tab_visible(model: &Model, width: u16) -> bool {
        let (_, viewport) = resolve_layout(model, width);
        viewport
            .visible
            .iter()
            .any(|item| item.index == model.active_document)
    }

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

    #[test]
    fn overflowing_tabs_show_scroll_indicators_and_keep_new_visible() {
        let model = many_documents(8);
        let (_, viewport) = resolve_layout(&model, 40);

        assert!(viewport.show_prev || viewport.show_next);
        assert!(!viewport.visible.is_empty());
        assert!(active_tab_visible(&model, 40));
    }

    #[test]
    fn sync_scroll_keeps_active_tab_visible() {
        let mut model = many_documents(8);
        model.document_tabs_scroll = 0;

        sync_scroll(&mut model, 40);

        assert!(active_tab_visible(&model, 40));
    }

    #[test]
    fn active_tab_stays_visible_on_a_narrow_terminal() {
        let model = many_documents(8);

        for width in [18, 22, 28, 34, 40] {
            assert!(
                active_tab_visible(&model, width),
                "active tab must render at width {width}"
            );
        }
    }

    #[test]
    fn narrow_terminal_still_renders_the_active_document_title() {
        let model = many_documents(8);
        let frame = render_to_string(&model, 22, 20);

        assert!(
            frame.contains("query-7.sql"),
            "expected active tab title in frame:\n{frame}"
        );
    }

    #[test]
    fn long_titles_are_truncated_in_the_tab_bar() {
        let mut model = Model::default();
        model.documents[0] = EditorDocument::new_unique(
            "very-long-query-name-that-should-not-overflow-the-tab-bar.sql",
            None,
            None,
        );

        let items = tab_items(&model, 20);

        assert!(items[0].label.contains('…'));
        assert!(UnicodeWidthStr::width(items[0].label.as_str()) <= 24);
        assert!(active_tab_visible(&model, 24));
    }
}
