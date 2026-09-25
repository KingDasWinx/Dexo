use std::time::Duration;

use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Value, json};

/// Cells longer than this are cut in the Markdown view; `structuredContent` keeps them
/// whole, within the profile's byte limit.
const CELL_CHARS: usize = 200;

#[derive(Clone, Debug, Default)]
pub struct RowsPage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
    pub bytes: u64,
    pub elapsed: Duration,
    pub next_offset: Option<u64>,
    pub title: Option<String>,
}

impl RowsPage {
    pub fn new(columns: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        let bytes = rows.iter().flatten().map(|cell| cell.len() as u64).sum();
        Self {
            columns,
            rows,
            bytes,
            ..Self::default()
        }
    }
}

/// A Markdown table for the model to read, and the same rows as `structuredContent` for
/// clients that parse. The result always says whether it was cut short (MCP-013).
pub fn rows_result(page: &RowsPage) -> CallToolResult {
    let mut text = String::new();
    if let Some(title) = &page.title {
        text.push_str(title);
        text.push_str("\n\n");
    }
    text.push_str(&markdown_table(&page.columns, &page.rows));
    text.push_str(&format!(
        "\n({} rows, {} ms)",
        page.rows.len(),
        page.elapsed.as_millis()
    ));
    if page.truncated {
        text.push_str("\nResult truncated at the profile's row or byte limit; narrow the query or select fewer columns.");
    }
    if let Some(offset) = page.next_offset {
        text.push_str(&format!("\nMore rows: call again with offset={offset}."));
    }
    text_result(
        text,
        json!({
            "columns": page.columns,
            "rows": page.rows,
            "row_count": page.rows.len(),
            "truncated": page.truncated,
            "bytes": page.bytes,
            "elapsed_ms": page.elapsed.as_millis() as u64,
            "next_offset": page.next_offset,
        }),
    )
}

pub fn text_result(text: impl Into<String>, data: Value) -> CallToolResult {
    let mut result = CallToolResult::success(vec![ContentBlock::text(text.into())]);
    result.structured_content = Some(data);
    result
}

pub fn markdown_table(columns: &[String], rows: &[Vec<String>]) -> String {
    let mut out = String::from("|");
    for column in columns {
        out.push_str(&format!(" {} |", cell(column)));
    }
    out.push_str("\n|");
    for _ in columns {
        out.push_str(" --- |");
    }
    out.push('\n');
    for row in rows {
        out.push('|');
        for value in row {
            out.push_str(&format!(" {} |", cell(value)));
        }
        out.push('\n');
    }
    out
}

fn cell(text: &str) -> String {
    let flat = text.replace('|', "\\|").replace(['\n', '\r'], " ");
    if flat.chars().count() > CELL_CHARS {
        format!("{}…", flat.chars().take(CELL_CHARS).collect::<String>())
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::markdown_table;

    #[test]
    fn cells_cannot_break_the_table() {
        let table = markdown_table(
            &["a".into()],
            &[vec!["x|y\nz".into()], vec!["w".repeat(300)]],
        );
        assert!(table.contains("x\\|y z"));
        assert!(table.contains('…'));
        assert_eq!(table.lines().count(), 4);
    }
}
