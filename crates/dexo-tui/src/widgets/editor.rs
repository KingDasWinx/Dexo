use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthChar;

use crate::model::{Focus, Model};
use crate::screens::editor::line_col_of;
use crate::theme::Role;

/// Columns the line numbers and the statement marker take, left of the text.
pub const GUTTER: u16 = 5;

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if model.active_document().kind.is_placeholder() {
        render_nothing_open(frame, area, model);
        return;
    }
    let doc = model.active_document();
    let title = if doc.is_dirty() {
        format!("SQL · {}*", doc.title)
    } else {
        format!("SQL · {}", doc.title)
    };
    if area.width < 2 || area.height < 2 {
        frame.render_widget(Paragraph::new(doc.text()), area);
        return;
    }
    let focused = model.effective_focus() == Focus::Editor;
    let block = crate::render::pane_block(model, &title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // ponytail: the whole buffer is still copied, split and scanned once per frame --
    // under a millisecond at 3 300 lines (benches/editor_navigation). Read the rope's
    // visible lines directly if a far larger script ever pushes it past the budget.
    let text = doc.text();
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };
    let gutter = GUTTER;
    let text_width = inner.width.saturating_sub(gutter);
    let sel = doc.selection();
    let cursor = doc.cursor();
    let stmt = current_statement_lines(&text, cursor);
    let sel_style = model
        .theme
        .style(Role::Selection, model.capabilities)
        .add_modifier(Modifier::REVERSED);
    let marker_style = model.theme.style(Role::Focus, model.capabilities);
    let muted = model.theme.gutter(model.capabilities);

    let mut rendered = Vec::new();
    let start = doc.viewport_line.min(lines.len().saturating_sub(1));
    let end = (start + inner.height as usize).min(lines.len());
    let mut char_at = char_index_at_line(&text, start);
    // Byte offsets for the window, found once per frame. The highlighter works in
    // bytes and the lines in chars; converting per character, from the start of the
    // document each time, is what made a frame cost as much as the document was long.
    let mut byte_at: usize = lines[..start].iter().map(|line| line.len() + 1).sum();
    let window_end = byte_at
        + lines[start..end]
            .iter()
            .map(|line| line.len() + 1)
            .sum::<usize>();
    // Only the spans that touch the window, still in the parser's order: the first one
    // containing a byte wins, and nested captures depend on that order.
    let window_highlights: Vec<&dexo_sql::HighlightSpan> = model
        .editor
        .highlights
        .iter()
        .filter(|span| span.byte_range.start < window_end && span.byte_range.end > byte_at)
        .collect();
    for (row, line) in lines[start..end].iter().enumerate() {
        let line_no = start + row + 1;
        let marker = if stmt.contains(&(start + row)) {
            "▸"
        } else {
            " "
        };
        let mut spans = vec![
            Span::styled(format!("{line_no:>4}"), muted),
            Span::styled(marker.to_string(), marker_style),
        ];
        let visible = visible_slice(line, doc.viewport_column, text_width as usize);
        let line_start = char_at;
        let line_end = line_start + line.chars().count();
        let visible_byte = byte_at
            + line
                .chars()
                .take(visible.skip_chars)
                .map(char::len_utf8)
                .sum::<usize>();
        if let Some(range) = &sel {
            spans.extend(selection_spans(
                &visible.text,
                line_start + visible.skip_chars,
                range,
                sel_style,
            ));
        } else {
            spans.extend(highlight_spans(
                &visible.text,
                visible_byte,
                &window_highlights,
            ));
        }
        rendered.push(Line::from(spans));
        char_at = line_end + 1;
        byte_at += line.len() + 1;
    }
    frame.render_widget(Paragraph::new(rendered), inner);

    if model.effective_focus() == Focus::Editor {
        let (line, col) = line_col_of(&text, cursor);
        if line >= start && line < end {
            let line_text = lines.get(line).copied().unwrap_or("");
            let x_off = display_width_range(line_text, doc.viewport_column, col);
            let x = inner.x + gutter + x_off.min(text_width.saturating_sub(1) as usize) as u16;
            let y = inner.y + (line - start) as u16;
            if x < inner.x + inner.width && y < inner.y + inner.height {
                frame.set_cursor_position(Position::new(x, y));
            }
        }
    }
}

/// What the editor shows when no document is open: the ways to get one. Typing works
/// too -- the first keystroke becomes a document of the active connection.
fn render_nothing_open(frame: &mut Frame, area: Rect, model: &Model) {
    let focused = model.effective_focus() == Focus::Editor;
    let block = crate::render::pane_block(model, "SQL", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let lines = [
        "No document open",
        "",
        "Ctrl+N  new query",
        "Ctrl+O  open a file",
        "or just start typing",
    ];
    let top = inner.height.saturating_sub(lines.len() as u16) / 2;
    // The hints are padded to one width so they centre as a block and their keys line
    // up, rather than each line centring on its own and leaving a ragged edge.
    let width = lines[2..].iter().map(|text| text.len()).max().unwrap_or(0);
    let body: Vec<Line> = std::iter::repeat_n(Line::raw(""), top as usize)
        .chain(lines.iter().enumerate().map(|(index, text)| match index {
            0 => Line::raw(*text).centered(),
            1 => Line::raw(""),
            _ => Line::styled(format!("{text:<width$}"), muted).centered(),
        }))
        .collect();
    frame.render_widget(Paragraph::new(body), inner);
}

pub fn char_index_at(model: &Model, area: Rect, x: u16, y: u16) -> Option<usize> {
    let inner = if area.width < 2 || area.height < 2 {
        area
    } else {
        ratatui::widgets::Block::bordered().inner(area)
    };
    if inner.width == 0 || inner.height == 0 {
        return None;
    }
    if x < inner.x || y < inner.y {
        return None;
    }
    let gutter = 5u16;
    let rel_y = y.saturating_sub(inner.y) as usize;
    let rel_x = x.saturating_sub(inner.x).saturating_sub(gutter) as usize;
    let doc = model.active_document();
    let text = doc.text();
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };
    let line_i = doc.viewport_line.saturating_add(rel_y);
    let line = *lines.get(line_i)?;
    let mut cols = 0usize;
    let mut chars = 0usize;
    for ch in line.chars() {
        if cols >= doc.viewport_column && cols.saturating_sub(doc.viewport_column) >= rel_x {
            break;
        }
        cols += UnicodeWidthChar::width(ch).unwrap_or(0);
        chars += 1;
    }
    Some(char_index_at_line(&text, line_i) + chars)
}

struct Visible {
    text: String,
    skip_chars: usize,
}

fn visible_slice(line: &str, skip_cols: usize, width: usize) -> Visible {
    let mut cols = 0;
    let mut skip_chars = 0;
    let mut out = String::new();
    for ch in line.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if cols + w <= skip_cols {
            cols += w;
            skip_chars += 1;
            continue;
        }
        if display_width(&out) + w > width {
            break;
        }
        out.push(ch);
        cols += w;
    }
    Visible {
        text: out,
        skip_chars,
    }
}

fn selection_spans(
    visible: &str,
    line_char_start: usize,
    range: &std::ops::Range<usize>,
    sel_style: Style,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut selected = false;
    for (offset, ch) in visible.chars().enumerate() {
        let index = line_char_start + offset;
        let now = index >= range.start && index < range.end;
        if now != selected && !buf.is_empty() {
            spans.push(span_owned(std::mem::take(&mut buf), selected, sel_style));
        }
        selected = now;
        buf.push(ch);
    }
    if !buf.is_empty() {
        spans.push(span_owned(buf, selected, sel_style));
    }
    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    spans
}

fn span_owned(text: String, selected: bool, sel_style: Style) -> Span<'static> {
    if selected {
        Span::styled(text, sel_style)
    } else {
        Span::raw(text)
    }
}

/// Styles one visible slice. `start_byte` is where the slice begins in the document,
/// and `highlights` is only what touches the frame -- the caller filters it once.
fn highlight_spans(
    visible: &str,
    start_byte: usize,
    highlights: &[&dexo_sql::HighlightSpan],
) -> Vec<Span<'static>> {
    if highlights.is_empty() {
        return vec![Span::raw(visible.to_string())];
    }
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut current = Style::default();
    let mut byte = start_byte;
    for ch in visible.chars() {
        let style = highlights
            .iter()
            .find(|span| byte >= span.byte_range.start && byte < span.byte_range.end)
            .map(|span| highlight_style(span.kind))
            .unwrap_or_default();
        byte += ch.len_utf8();
        if style != current && !buf.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut buf), current));
        }
        current = style;
        buf.push(ch);
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, current));
    }
    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    spans
}

fn highlight_style(kind: dexo_sql::Highlight) -> Style {
    use ratatui::style::Color;
    match kind {
        dexo_sql::Highlight::Keyword => Style::default().fg(Color::Cyan),
        dexo_sql::Highlight::String => Style::default().fg(Color::Green),
        dexo_sql::Highlight::Comment => Style::default().fg(Color::DarkGray),
        dexo_sql::Highlight::Number => Style::default().fg(Color::Yellow),
        dexo_sql::Highlight::Function => Style::default().fg(Color::Magenta),
        dexo_sql::Highlight::Identifier | dexo_sql::Highlight::Other => Style::default(),
    }
}

fn current_statement_lines(text: &str, cursor: usize) -> Vec<usize> {
    let byte = text.chars().take(cursor).map(char::len_utf8).sum();
    let Some(span) = dexo_sql::statement_at(text, byte) else {
        return Vec::new();
    };
    let start = text[..span.byte_range.start].matches('\n').count();
    let end = text[..span.byte_range.end.min(text.len())]
        .matches('\n')
        .count();
    (start..=end).collect()
}

fn char_index_at_line(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut current = 0;
    for (index, ch) in text.chars().enumerate() {
        if ch == '\n' {
            current += 1;
            if current == line {
                return index + 1;
            }
        }
    }
    text.chars().count()
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum()
}

fn display_width_range(line: &str, from_col: usize, to_char: usize) -> usize {
    let mut cols = 0;
    for (index, ch) in line.chars().enumerate() {
        if index >= to_char {
            break;
        }
        cols += UnicodeWidthChar::width(ch).unwrap_or(0);
    }
    cols.saturating_sub(from_col)
}

#[cfg(test)]
mod tests {
    use super::highlight_spans;
    use ratatui::text::Span;

    /// The highlighter as it was: for every character, walk the document from the start
    /// to find its byte, then scan every span for the first that holds it. Correct and
    /// quadratic -- kept here only as the oracle the windowed version must agree with.
    fn reference(
        visible: &str,
        line_char_start: usize,
        full: &str,
        highlights: &[dexo_sql::HighlightSpan],
    ) -> Vec<(String, ratatui::style::Style)> {
        let char_to_byte =
            |chars: usize| -> usize { full.chars().take(chars).map(char::len_utf8).sum() };
        let mut out: Vec<(String, ratatui::style::Style)> = Vec::new();
        for (offset, ch) in visible.chars().enumerate() {
            let byte = char_to_byte(line_char_start + offset);
            let style = highlights
                .iter()
                .find(|span| byte >= span.byte_range.start && byte < span.byte_range.end)
                .map(|span| super::highlight_style(span.kind))
                .unwrap_or_default();
            match out.last_mut() {
                Some((text, last)) if *last == style => text.push(ch),
                _ => out.push((ch.to_string(), style)),
            }
        }
        out
    }

    fn flatten(spans: Vec<Span<'static>>) -> Vec<(String, ratatui::style::Style)> {
        let mut out: Vec<(String, ratatui::style::Style)> = Vec::new();
        for span in spans {
            for ch in span.content.chars() {
                match out.last_mut() {
                    Some((text, last)) if *last == span.style => text.push(ch),
                    _ => out.push((ch.to_string(), span.style)),
                }
            }
        }
        out
    }

    /// Every line of every document, at several horizontal scroll offsets, must style
    /// exactly as the per-character original did.
    #[test]
    fn the_windowed_highlighter_matches_the_original() {
        let documents = [
            "select id, 'texto com acentuação' from t -- comentário é\nwhere a = 1;",
            "create table t (\n  preço numeric(10,2) default 0, -- ção\n  nome text\n);",
            "/* bloco\n   de várias linhas */ select 1;\nselect 'fim';",
            include_str!("../../../dexo-sql/tests/fixtures_schema_vendas.sql"),
        ];
        let mut parser = dexo_sql::ParserService::new(dexo_sql::Dialect::Postgres);
        for sql in documents {
            let highlights = parser.parse_edited("", sql).highlights;
            let lines: Vec<&str> = sql.split('\n').collect();
            let mut char_at = 0;
            let mut byte_at = 0;
            for line in &lines {
                for skip in [0usize, 3, 11] {
                    let visible: String = line.chars().skip(skip).collect();
                    let visible_byte =
                        byte_at + line.chars().take(skip).map(char::len_utf8).sum::<usize>();
                    let window: Vec<&dexo_sql::HighlightSpan> = highlights.iter().collect();
                    assert_eq!(
                        flatten(highlight_spans(&visible, visible_byte, &window)),
                        reference(
                            &visible,
                            char_at + skip.min(line.chars().count()),
                            sql,
                            &highlights
                        ),
                        "line {line:?} scrolled by {skip}"
                    );
                }
                char_at += line.chars().count() + 1;
                byte_at += line.len() + 1;
            }

            // The window the renderer actually filters to: a band of lines out of the
            // middle, which keeps spans that start above it and run into it.
            let (first, last) = (lines.len() / 3, (lines.len() / 3 + 8).min(lines.len()));
            let window_start: usize = lines[..first].iter().map(|line| line.len() + 1).sum();
            let window_end = window_start
                + lines[first..last]
                    .iter()
                    .map(|line| line.len() + 1)
                    .sum::<usize>();
            let window: Vec<&dexo_sql::HighlightSpan> = highlights
                .iter()
                .filter(|span| {
                    span.byte_range.start < window_end && span.byte_range.end > window_start
                })
                .collect();
            let mut char_at: usize = lines[..first]
                .iter()
                .map(|line| line.chars().count() + 1)
                .sum();
            let mut byte_at = window_start;
            for line in &lines[first..last] {
                assert_eq!(
                    flatten(highlight_spans(line, byte_at, &window)),
                    reference(line, char_at, sql, &highlights),
                    "windowed line {line:?}"
                );
                char_at += line.chars().count() + 1;
                byte_at += line.len() + 1;
            }
        }
    }
}
