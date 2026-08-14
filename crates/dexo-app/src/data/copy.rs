use dexo_driver_api::DbValue;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopyFormat {
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
}

pub fn copy_selection(
    columns: &[String],
    rows: &[Vec<DbValue>],
    format: CopyFormat,
    dialect: SqlDialect,
) -> Result<String, String> {
    for row in rows {
        for value in row {
            let _ = cell(value)?;
        }
    }
    match format {
        CopyFormat::Text => Ok(delimited(columns, rows, ' ')),
        CopyFormat::Csv => Ok(delimited(columns, rows, ',')),
        CopyFormat::Tsv => Ok(delimited(columns, rows, '\t')),
        CopyFormat::Json => json(columns, rows),
        CopyFormat::Markdown => Ok(markdown(columns, rows)),
        CopyFormat::Sql => Ok(sql(columns, rows, dialect)),
    }
}

fn cell(value: &DbValue) -> Result<String, String> {
    match value {
        DbValue::Bytes(bytes) if is_truncated_marker(bytes) => {
            Err("refusing to copy truncated bytes as complete".into())
        }
        _ => Ok(display(value)),
    }
}

fn is_truncated_marker(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\0TRUNC")
}

fn display(value: &DbValue) -> String {
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

fn delimited(columns: &[String], rows: &[Vec<DbValue>], sep: char) -> String {
    let mut out = columns.join(&sep.to_string());
    out.push('\n');
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .map(|value| match value {
                DbValue::Null => "\\N".into(),
                DbValue::Text(text) if text.is_empty() => String::new(),
                other => display(other),
            })
            .collect();
        out.push_str(&cells.join(&sep.to_string()));
        out.push('\n');
    }
    out
}

fn json(columns: &[String], rows: &[Vec<DbValue>]) -> Result<String, String> {
    let objects: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let mut map = serde_json::Map::new();
            for (name, value) in columns.iter().zip(row.iter()) {
                map.insert(name.clone(), json_value(value));
            }
            serde_json::Value::Object(map)
        })
        .collect();
    Ok(serde_json::to_string(&objects).unwrap_or_default())
}

fn json_value(value: &DbValue) -> serde_json::Value {
    match value {
        DbValue::Null => serde_json::Value::Null,
        DbValue::Bool(v) => serde_json::Value::Bool(*v),
        DbValue::I64(v) => serde_json::json!(*v),
        DbValue::U64(v) => serde_json::json!(*v),
        DbValue::Decimal(v) | DbValue::Text(v) | DbValue::Json(v) => {
            serde_json::Value::String(v.clone())
        }
        DbValue::Bytes(v) => serde_json::Value::String(display(&DbValue::Bytes(v.clone()))),
        DbValue::Native { text, .. } => serde_json::Value::String(text.clone()),
    }
}

fn markdown(columns: &[String], rows: &[Vec<DbValue>]) -> String {
    let mut out = format!("| {} |\n", columns.join(" | "));
    out.push_str(&format!(
        "| {} |\n",
        columns
            .iter()
            .map(|_| "---")
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    for row in rows {
        let cells: Vec<String> = row.iter().map(display).collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out
}

fn sql(columns: &[String], rows: &[Vec<DbValue>], dialect: SqlDialect) -> String {
    let ident = |name: &str| match dialect {
        SqlDialect::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
        SqlDialect::Mysql => format!("`{}`", name.replace('`', "``")),
    };
    let cols = columns
        .iter()
        .map(|name| ident(name))
        .collect::<Vec<_>>()
        .join(", ");
    rows.iter()
        .map(|row| {
            let values = row
                .iter()
                .map(|value| sql_literal(value, dialect))
                .collect::<Vec<_>>()
                .join(", ");
            format!("INSERT INTO tbl ({cols}) VALUES ({values});")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sql_literal(value: &DbValue, dialect: SqlDialect) -> String {
    match value {
        DbValue::Null => "NULL".into(),
        DbValue::Bool(v) => match dialect {
            SqlDialect::Postgres => if *v { "TRUE" } else { "FALSE" }.into(),
            SqlDialect::Mysql => if *v { "1" } else { "0" }.into(),
        },
        DbValue::I64(v) => v.to_string(),
        DbValue::U64(v) => v.to_string(),
        DbValue::Decimal(v) => v.clone(),
        DbValue::Text(v) | DbValue::Json(v) | DbValue::Native { text: v, .. } => {
            format!("'{}'", v.replace('\'', "''"))
        }
        DbValue::Bytes(v) => match dialect {
            SqlDialect::Postgres => format!("'\\x{}'", hex(v)),
            SqlDialect::Mysql => format!("X'{}'", hex(v)),
        },
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{CopyFormat, SqlDialect, copy_selection};
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
}
