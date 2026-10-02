use dexo_driver_api::DbValue;

use crate::transfer::codec::{FormatOptions, TransferFormat, encode_document, sql_insert};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopyFormat {
    /// The values alone: no header, a tab between cells, one row per line. A single
    /// cell copies as exactly its value.
    Value,
    Text,
    Csv,
    Tsv,
    Json,
    Markdown,
    Sql,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqlDialect {
    Postgres,
    Mysql,
    Sqlite,
    Duckdb,
}

impl From<dexo_sql::Dialect> for SqlDialect {
    fn from(dialect: dexo_sql::Dialect) -> Self {
        match dialect {
            dexo_sql::Dialect::Postgres => Self::Postgres,
            dexo_sql::Dialect::Mysql => Self::Mysql,
            dexo_sql::Dialect::Sqlite => Self::Sqlite,
            dexo_sql::Dialect::Duckdb => Self::Duckdb,
        }
    }
}

pub fn copy_selection(
    columns: &[String],
    rows: &[Vec<DbValue>],
    format: CopyFormat,
    dialect: SqlDialect,
) -> Result<String, String> {
    copy_selection_of(columns, rows, format, dialect, None)
}

/// `copy_selection` for rows of a known table: `table`, unquoted (`schema.table`), is
/// what a copied INSERT inserts into.
pub fn copy_selection_of(
    columns: &[String],
    rows: &[Vec<DbValue>],
    format: CopyFormat,
    dialect: SqlDialect,
    table: Option<&str>,
) -> Result<String, String> {
    for row in rows {
        for value in row {
            let _ = cell(value)?;
        }
    }
    match format {
        CopyFormat::Value => Ok(rows
            .iter()
            .map(|row| row.iter().map(display_value).collect::<Vec<_>>().join("\t"))
            .collect::<Vec<_>>()
            .join("\n")),
        // Text pastes into a spreadsheet as columns: tab-separated, like TSV.
        CopyFormat::Text | CopyFormat::Tsv => delimited(columns, rows, TransferFormat::Tsv),
        CopyFormat::Csv => delimited(columns, rows, TransferFormat::Csv),
        CopyFormat::Json => json(columns, rows),
        CopyFormat::Markdown => Ok(markdown(columns, rows)),
        CopyFormat::Sql => Ok(sql(columns, rows, dialect, table.unwrap_or("tbl"))),
    }
}

fn cell(value: &DbValue) -> Result<String, String> {
    match value {
        DbValue::Bytes(bytes) if is_truncated_marker(bytes) => {
            Err("refusing to copy truncated bytes as complete".into())
        }
        _ => Ok(display_value(value)),
    }
}

fn is_truncated_marker(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\0TRUNC")
}

pub fn display_value(value: &DbValue) -> String {
    match value {
        DbValue::Null => "NULL".into(),
        DbValue::Bool(v) => v.to_string(),
        DbValue::I64(v) => v.to_string(),
        DbValue::U64(v) => v.to_string(),
        DbValue::Decimal(v) | DbValue::Text(v) | DbValue::Json(v) => v.clone(),
        DbValue::Bytes(v) => {
            if v.is_empty() {
                "\\x".into()
            } else {
                format!(
                    "\\x{}",
                    v.iter().map(|b| format!("{b:02x}")).collect::<String>()
                )
            }
        }
        DbValue::Native { text, .. } => text.clone(),
    }
}

/// CSV and TSV as an export writes them: a value holding the separator, a quote or a
/// line break is quoted, and NULL is `\N`, apart from an empty string.
fn delimited(
    columns: &[String],
    rows: &[Vec<DbValue>],
    format: TransferFormat,
) -> Result<String, String> {
    let bytes = encode_document(format, &FormatOptions::default(), columns, rows)?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

/// The rows as an array of objects whose keys keep the columns' order: `serde_json`'s
/// map sorts them, so the objects are written key by key.
fn json(columns: &[String], rows: &[Vec<DbValue>]) -> Result<String, String> {
    let pretty = |value: &serde_json::Value| {
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())
    };
    let mut objects = Vec::with_capacity(rows.len());
    for row in rows {
        let mut members = Vec::with_capacity(columns.len());
        for (name, value) in columns.iter().zip(row.iter()) {
            let key = pretty(&serde_json::Value::String(name.clone()))?;
            let value = pretty(&json_value(value))?.replace('\n', "\n    ");
            members.push(format!("    {key}: {value}"));
        }
        objects.push(if members.is_empty() {
            "  {}".to_string()
        } else {
            format!("  {{\n{}\n  }}", members.join(",\n"))
        });
    }
    Ok(if objects.is_empty() {
        "[]".to_string()
    } else {
        format!("[\n{}\n]", objects.join(",\n"))
    })
}

fn json_value(value: &DbValue) -> serde_json::Value {
    match value {
        DbValue::Null => serde_json::Value::Null,
        DbValue::Bool(v) => serde_json::Value::Bool(*v),
        DbValue::I64(v) => serde_json::json!(*v),
        DbValue::U64(v) => serde_json::json!(*v),
        DbValue::Decimal(v) | DbValue::Text(v) => serde_json::Value::String(v.clone()),
        DbValue::Json(v) => {
            serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.clone()))
        }
        DbValue::Bytes(v) => serde_json::Value::String(display_value(&DbValue::Bytes(v.clone()))),
        DbValue::Native { text, .. } => serde_json::Value::String(text.clone()),
    }
}

/// A cell stays on its row: a line break becomes `<br>` and a bar is escaped.
fn markdown_cell(text: &str) -> String {
    text.replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace(['\n', '\r'], "<br>")
}

fn markdown(columns: &[String], rows: &[Vec<DbValue>]) -> String {
    let names: Vec<String> = columns.iter().map(|name| markdown_cell(name)).collect();
    let mut out = format!("| {} |\n", names.join(" | "));
    out.push_str(&format!(
        "| {} |\n",
        columns
            .iter()
            .map(|_| "---")
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .map(|value| markdown_cell(&display_value(value)))
            .collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out
}

/// One INSERT per row into `table`, each line ended like the other formats' rows.
fn sql(columns: &[String], rows: &[Vec<DbValue>], dialect: SqlDialect, table: &str) -> String {
    rows.iter()
        .map(|row| format!("{}\n", sql_insert(table, columns, row, dialect)))
        .collect()
}

/// `value` as a literal of `dialect`: for a copied INSERT and an exported one alike.
pub(crate) fn sql_literal(value: &DbValue, dialect: SqlDialect) -> String {
    match value {
        DbValue::Null => "NULL".into(),
        DbValue::Bool(v) => match dialect {
            SqlDialect::Postgres | SqlDialect::Duckdb => if *v { "TRUE" } else { "FALSE" }.into(),
            SqlDialect::Mysql | SqlDialect::Sqlite => if *v { "1" } else { "0" }.into(),
        },
        DbValue::I64(v) => v.to_string(),
        DbValue::U64(v) => v.to_string(),
        DbValue::Decimal(v) => v.clone(),
        DbValue::Text(v) | DbValue::Json(v) | DbValue::Native { text: v, .. } => match dialect {
            SqlDialect::Mysql => dexo_driver_api::mysql_string_literal(v),
            SqlDialect::Postgres | SqlDialect::Sqlite | SqlDialect::Duckdb => {
                format!("'{}'", v.replace('\'', "''"))
            }
        },
        DbValue::Bytes(v) => match dialect {
            SqlDialect::Postgres => format!("'\\x{}'", hex(v)),
            // DuckDB reads `'\x01'` as escaped bytes, not as one hex string.
            SqlDialect::Duckdb => format!("from_hex('{}')", hex(v)),
            SqlDialect::Mysql | SqlDialect::Sqlite => format!("X'{}'", hex(v)),
        },
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn value_copies_cells_without_a_header() {
        use super::{CopyFormat, SqlDialect, copy_selection};
        use dexo_driver_api::DbValue;
        let one = copy_selection(
            &["n".into()],
            &[vec![DbValue::I64(2)]],
            CopyFormat::Value,
            SqlDialect::Postgres,
        );
        assert_eq!(one.unwrap(), "2");
        let many = copy_selection(
            &["a".into(), "b".into()],
            &[
                vec![DbValue::I64(1), DbValue::Text("x".into())],
                vec![DbValue::Null, DbValue::Text("y".into())],
            ],
            CopyFormat::Value,
            SqlDialect::Postgres,
        );
        assert_eq!(many.unwrap(), "1\tx\nNULL\ty");
    }

    use super::{CopyFormat, SqlDialect, copy_selection, copy_selection_of};
    use dexo_driver_api::DbValue;

    #[test]
    fn copy_distinguishes_null_empty_text_and_empty_bytes() {
        let columns = vec!["a".into()];
        let json = copy_selection(
            &columns,
            &[vec![DbValue::Null]],
            CopyFormat::Json,
            SqlDialect::Postgres,
        )
        .unwrap();
        assert!(json.contains("null"));
        let empty = copy_selection(
            &columns,
            &[vec![DbValue::Text(String::new())]],
            CopyFormat::Json,
            SqlDialect::Postgres,
        )
        .unwrap();
        assert!(empty.contains("\"\""));
        let bytes = copy_selection(
            &columns,
            &[vec![DbValue::Bytes(vec![])]],
            CopyFormat::Text,
            SqlDialect::Postgres,
        )
        .unwrap();
        assert!(bytes.contains("\\x"));
        let truncated = copy_selection(
            &columns,
            &[vec![DbValue::Bytes(b"\0TRUNC".to_vec())]],
            CopyFormat::Json,
            SqlDialect::Postgres,
        );
        assert!(truncated.is_err());
        let csv = copy_selection(
            &["a".into(), "b".into()],
            &[vec![DbValue::Null, DbValue::Text(String::new())]],
            CopyFormat::Csv,
            SqlDialect::Postgres,
        )
        .unwrap();
        assert!(csv.contains("\\N"));
        let sql = copy_selection(
            &["id".into()],
            &[vec![DbValue::Text("O'Reilly".into())]],
            CopyFormat::Sql,
            SqlDialect::Postgres,
        )
        .unwrap();
        assert!(sql.contains("\"id\""));
        assert!(sql.contains("'O''Reilly'"));
        assert!(!sql.contains("'O'Reilly'"));
    }

    fn awkward_rows() -> (Vec<String>, Vec<Vec<DbValue>>) {
        (
            vec!["id".into(), "name".into(), "note".into()],
            vec![
                vec![
                    DbValue::I64(1),
                    DbValue::Text("Ana, \"the\" Silva".into()),
                    DbValue::Null,
                ],
                vec![
                    DbValue::I64(2),
                    DbValue::Text("multi\nline|name".into()),
                    DbValue::Text(String::new()),
                ],
            ],
        )
    }

    /// A comma, a quote or a line break inside a value no longer splits the record.
    #[test]
    fn csv_quotes_what_needs_quoting() {
        let (columns, rows) = awkward_rows();
        let csv = copy_selection(&columns, &rows, CopyFormat::Csv, SqlDialect::Postgres).unwrap();
        assert_eq!(
            csv,
            "id,name,note\n1,\"Ana, \"\"the\"\" Silva\",\\N\n2,\"multi\nline|name\",\n"
        );
    }

    /// Text is the one that pastes into a spreadsheet as columns.
    #[test]
    fn text_is_tab_separated() {
        let (columns, rows) = awkward_rows();
        let text = copy_selection(&columns, &rows, CopyFormat::Text, SqlDialect::Postgres).unwrap();
        assert!(text.starts_with("id\tname\tnote\n1\t"), "{text}");
    }

    #[test]
    fn json_keeps_the_columns_order() {
        let columns = vec!["id".into(), "name".into(), "created_at".into()];
        let rows = vec![vec![
            DbValue::I64(1),
            DbValue::Text("x".into()),
            DbValue::Json(r#"{"b":1}"#.into()),
        ]];
        let json = copy_selection(&columns, &rows, CopyFormat::Json, SqlDialect::Postgres).unwrap();
        assert_eq!(
            json,
            "[\n  {\n    \"id\": 1,\n    \"name\": \"x\",\n    \"created_at\": {\n      \"b\": 1\n    }\n  }\n]"
        );
        let empty = copy_selection(&columns, &[], CopyFormat::Json, SqlDialect::Postgres).unwrap();
        assert_eq!(empty, "[]");
    }

    #[test]
    fn markdown_cells_stay_on_their_row() {
        let (columns, rows) = awkward_rows();
        let table =
            copy_selection(&columns, &rows, CopyFormat::Markdown, SqlDialect::Postgres).unwrap();
        assert_eq!(table.lines().count(), 4, "{table}");
        assert!(table.contains("multi<br>line\\|name"), "{table}");
    }

    /// The INSERTs name the table the rows came from, and each ends its line.
    #[test]
    fn sql_names_the_table_it_is_given() {
        let (columns, rows) = awkward_rows();
        let sql = copy_selection_of(
            &columns,
            &rows,
            CopyFormat::Sql,
            SqlDialect::Postgres,
            Some("public.customers"),
        )
        .unwrap();
        assert!(
            sql.starts_with("INSERT INTO \"public\".\"customers\" (\"id\", \"name\", \"note\")"),
            "{sql}"
        );
        assert!(sql.ends_with(";\n"), "{sql}");
        assert_eq!(sql.matches("INSERT INTO").count(), 2);
    }

    #[test]
    fn copy_json_embeds_json_columns() {
        let json = copy_selection(
            &["items".into()],
            &[vec![DbValue::Json(
                r#"[{"product_id":"abc","quantity":1}]"#.into(),
            )]],
            CopyFormat::Json,
            SqlDialect::Postgres,
        )
        .unwrap();
        assert!(json.contains("\"product_id\": \"abc\""));
        assert!(json.contains("\"quantity\": 1"));
        assert!(!json.contains("\\\"product_id\\\""));
    }

    /// MySQL reads a backslash in a literal as an escape: copied and exported INSERTs
    /// both escape it, so `\'` cannot end the literal and run what follows.
    #[test]
    fn mysql_inserts_escape_backslashes() {
        let rows = [vec![
            DbValue::Text("a\\'); DROP TABLE victim2; -- ".into()),
            DbValue::Text("C:\\new\\table".into()),
        ]];
        let columns = ["v".to_string(), "path".to_string()];
        let want = "VALUES ('a\\\\''); DROP TABLE victim2; -- ', 'C:\\\\new\\\\table');";
        let copied = copy_selection(&columns, &rows, CopyFormat::Sql, SqlDialect::Mysql).unwrap();
        assert!(copied.trim_end().ends_with(want), "{copied}");
        let options = crate::transfer::codec::FormatOptions {
            dialect: SqlDialect::Mysql,
            ..Default::default()
        };
        let exported = crate::transfer::codec::encode_document(
            crate::transfer::codec::TransferFormat::Sql,
            &options,
            &columns,
            &rows,
        )
        .unwrap();
        let exported = String::from_utf8(exported).unwrap();
        assert!(exported.trim_end().ends_with(want), "{exported}");
        // Postgres and SQLite take a backslash as itself.
        let postgres =
            copy_selection(&columns, &rows, CopyFormat::Sql, SqlDialect::Postgres).unwrap();
        assert!(postgres.contains("'C:\\new\\table'"), "{postgres}");
    }
}
