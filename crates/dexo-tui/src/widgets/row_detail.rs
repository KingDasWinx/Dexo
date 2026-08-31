use dexo_app::data::pretty_json;
use dexo_driver_api::DbValue;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::capabilities::TerminalCapabilities;
use crate::model::{GridCell, GridModel, append_field_detail, format_value, wrap_display_text};
use crate::theme::{Role, Theme};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowDetailValue {
    Text(String),
    Json(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowDetailField {
    pub name: String,
    pub value: RowDetailValue,
}

struct JsonStyles {
    key: Style,
    string: Style,
    number: Style,
    literal: Style,
    punct: Style,
    base: Style,
}

pub fn row_detail_fields(grid: &GridModel, row: usize) -> Vec<RowDetailField> {
    let Some(values) = grid.rows().get(row) else {
        return vec![RowDetailField {
            name: "(row)".into(),
            value: RowDetailValue::Text("(empty row)".into()),
        }];
    };
    let mut fields = Vec::new();
    for (col, column) in grid.columns().iter().enumerate() {
        fields.push(RowDetailField {
            name: column.name.clone(),
            value: classify_detail_value(grid, row, col, values.get(col), &column.type_name),
        });
    }
    if fields.is_empty() {
        fields.push(RowDetailField {
            name: "(columns)".into(),
            value: RowDetailValue::Text("(no columns)".into()),
        });
    }
    fields
}

pub fn row_detail_lines(
    fields: &[RowDetailField],
    wrap_width: usize,
    theme: &Theme,
    caps: TerminalCapabilities,
) -> Vec<Line<'static>> {
    let label_style = theme.style(Role::Muted, caps).add_modifier(Modifier::BOLD);
    let value_style = theme.style(Role::Foreground, caps);
    let badge_style = theme.style(Role::Development, caps);
    let json_styles = json_styles(theme, caps);
    let mut lines = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        match &field.value {
            RowDetailValue::Text(text) => {
                append_text_field(
                    &mut lines,
                    &field.name,
                    text,
                    wrap_width,
                    label_style,
                    value_style,
                );
            }
            RowDetailValue::Json(text) => {
                append_json_field(
                    &mut lines,
                    &field.name,
                    text,
                    wrap_width,
                    label_style,
                    badge_style,
                    &json_styles,
                );
            }
        }
        if index + 1 < fields.len() {
            lines.push(Line::from(""));
        }
    }
    lines
}

fn classify_detail_value(
    grid: &GridModel,
    row: usize,
    col: usize,
    value: Option<&DbValue>,
    type_name: &str,
) -> RowDetailValue {
    match grid.cell_at(row, col) {
        Some(GridCell::Spool { path, total, .. }) => {
            let bytes = std::fs::read(path).unwrap_or_default();
            if bytes.is_empty() {
                return RowDetailValue::Text(format!("<spooled {total} bytes>"));
            }
            let text = String::from_utf8(bytes).unwrap_or_else(|_| format!("<{total} bytes>"));
            classify_text_value(&text, type_name)
        }
        Some(GridCell::Remote(_)) => RowDetailValue::Text("<remote value>".into()),
        Some(GridCell::Inline(value)) => classify_db_value(value, type_name),
        None => value
            .map(|value| classify_db_value(value, type_name))
            .unwrap_or_else(|| RowDetailValue::Text("NULL".into())),
    }
}

fn classify_db_value(value: &DbValue, type_name: &str) -> RowDetailValue {
    match value {
        DbValue::Json(text) => RowDetailValue::Json(text.clone()),
        DbValue::Text(text) => classify_text_value(text, type_name),
        DbValue::Native { text, .. } => classify_text_value(text, type_name),
        other => RowDetailValue::Text(format_value(other)),
    }
}

fn classify_text_value(text: &str, type_name: &str) -> RowDetailValue {
    if type_name.to_ascii_lowercase().contains("json") || looks_like_json(text) {
        if serde_json::from_str::<serde_json::Value>(text).is_ok() {
            return RowDetailValue::Json(text.to_string());
        }
    }
    RowDetailValue::Text(text.to_string())
}

fn looks_like_json(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

fn json_styles(theme: &Theme, caps: TerminalCapabilities) -> JsonStyles {
    JsonStyles {
        key: theme
            .style(Role::Development, caps)
            .add_modifier(Modifier::BOLD),
        string: theme.style(Role::Success, caps),
        number: theme.style(Role::Warning, caps),
        literal: theme
            .style(Role::Muted, caps)
            .add_modifier(Modifier::ITALIC),
        punct: theme.style(Role::Border, caps),
        base: theme.style(Role::Foreground, caps),
    }
}

fn append_text_field(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    value: &str,
    width: usize,
    label_style: Style,
    value_style: Style,
) {
    let mut plain = Vec::new();
    append_field_detail(&mut plain, name, value, width);
    for line in plain {
        if let Some((label, rest)) = line.split_once(": ") {
            if rest.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    format!("{label}: "),
                    label_style,
                )]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(format!("{label}: "), label_style),
                    Span::styled(rest.to_string(), value_style),
                ]));
            }
            continue;
        }
        lines.push(Line::from(Span::styled(line, value_style)));
    }
}

fn append_json_field(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    json: &str,
    wrap_width: usize,
    label_style: Style,
    badge_style: Style,
    styles: &JsonStyles,
) {
    lines.push(Line::from(vec![
        Span::styled(name.to_string(), label_style),
        Span::styled("  json", badge_style),
    ]));
    let pretty = pretty_json(json);
    for line in pretty.lines() {
        let highlighted = highlight_json_line(line, styles);
        if wrap_width == 0 {
            lines.push(indent_line(highlighted, 2));
            continue;
        }
        let plain = line_content(line);
        if unicode_width::UnicodeWidthStr::width(plain.as_str()) + 2 <= wrap_width {
            lines.push(indent_line(highlighted, 2));
            continue;
        }
        for wrapped in wrap_display_text(line, wrap_width.saturating_sub(2)) {
            lines.push(indent_line(highlight_json_line(&wrapped, styles), 2));
        }
    }
}

fn indent_line(line: Line<'static>, spaces: usize) -> Line<'static> {
    let pad = " ".repeat(spaces);
    let mut spans = vec![Span::raw(pad)];
    spans.extend(line.spans);
    Line::from(spans)
}

fn line_content(line: &str) -> String {
    line.to_string()
}

fn highlight_json_line(line: &str, styles: &JsonStyles) -> Line<'static> {
    let mut spans = Vec::new();
    let mut index = 0;
    while index < line.len() {
        let rest = &line[index..];
        let Some(ch) = rest.chars().next() else {
            break;
        };
        if ch.is_whitespace() {
            let width = ch.len_utf8();
            spans.push(Span::styled(
                line[index..index + width].to_string(),
                styles.base,
            ));
            index += width;
            continue;
        }
        if ch == '"' {
            let (token, next, is_key) = read_json_string(line, index);
            let style = if is_key { styles.key } else { styles.string };
            spans.push(Span::styled(token, style));
            index = next;
            continue;
        }
        if ch == '-' || ch.is_ascii_digit() {
            let (token, next) = read_number(line, index);
            spans.push(Span::styled(token, styles.number));
            index = next;
            continue;
        }
        if rest.starts_with("true") {
            spans.push(Span::styled("true".to_string(), styles.literal));
            index += 4;
            continue;
        }
        if rest.starts_with("false") {
            spans.push(Span::styled("false".to_string(), styles.literal));
            index += 5;
            continue;
        }
        if rest.starts_with("null") {
            spans.push(Span::styled("null".to_string(), styles.literal));
            index += 4;
            continue;
        }
        if matches!(ch, '{' | '}' | '[' | ']' | ',' | ':') {
            spans.push(Span::styled(ch.to_string(), styles.punct));
            index += ch.len_utf8();
            continue;
        }
        let width = ch.len_utf8();
        spans.push(Span::styled(
            line[index..index + width].to_string(),
            styles.base,
        ));
        index += width;
    }
    Line::from(spans)
}

fn read_json_string(line: &str, start: usize) -> (String, usize, bool) {
    let mut index = start + 1;
    let mut escaped = false;
    while index < line.len() {
        let ch = line[index..].chars().next().unwrap();
        if escaped {
            escaped = false;
            index += ch.len_utf8();
            continue;
        }
        if ch == '\\' {
            escaped = true;
            index += 1;
            continue;
        }
        if ch == '"' {
            index += 1;
            let token = line[start..index].to_string();
            let mut tail = index;
            while let Some(ch) = line[tail..].chars().next() {
                if ch.is_whitespace() {
                    tail += ch.len_utf8();
                } else {
                    break;
                }
            }
            let is_key = line[tail..].starts_with(':');
            return (token, index, is_key);
        }
        index += ch.len_utf8();
    }
    (line[start..].to_string(), line.len(), false)
}

fn read_number(line: &str, start: usize) -> (String, usize) {
    let mut index = start;
    if line[index..].starts_with('-') {
        index += 1;
    }
    while index < line.len() {
        let ch = line[index..].chars().next().unwrap();
        if ch.is_ascii_digit() || matches!(ch, '.' | 'e' | 'E' | '+' | '-') {
            index += ch.len_utf8();
        } else {
            break;
        }
    }
    (line[start..index].to_string(), index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::TerminalCapabilities;
    use crate::theme::builtin_dark;

    #[test]
    fn json_field_is_pretty_printed_and_highlighted() {
        let theme = builtin_dark();
        let caps = TerminalCapabilities::detect();
        let fields = vec![RowDetailField {
            name: "payload".into(),
            value: RowDetailValue::Json(r#"{"id":1,"active":true,"note":null}"#.into()),
        }];
        let lines = row_detail_lines(&fields, 60, &theme, caps);
        assert!(lines.len() >= 3);
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|span| span.content.contains("payload"))
        );
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|span| span.content.contains("json"))
        );
        let rendered: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.clone()))
            .collect();
        assert!(rendered.contains("\"id\""));
        assert!(rendered.contains("true"));
        assert!(rendered.contains("null"));
        assert!(rendered.contains('\n') || lines.len() > 2);
    }

    #[test]
    fn text_json_in_text_column_is_detected() {
        let value = classify_text_value(r#"{"a":1}"#, "text");
        assert_eq!(value, RowDetailValue::Json(r#"{"a":1}"#.to_string()));
    }
}
