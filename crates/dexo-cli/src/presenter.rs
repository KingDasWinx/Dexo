use dexo_driver_api::{DbValue, QueryEvent};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Tsv,
    Json,
    Jsonl,
}

pub fn present(
    format: OutputFormat,
    events: &[QueryEvent],
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    for event in events {
        match event {
            QueryEvent::Columns(cols) => columns = cols.iter().map(|c| c.name.clone()).collect(),
            QueryEvent::Rows(batch) => rows.extend(batch.rows.iter().cloned()),
            QueryEvent::Notice { message } => writeln!(stderr, "{message}")?,
            QueryEvent::Finished { .. } => {}
        }
    }
    match format {
        OutputFormat::Jsonl => write_jsonl(&columns, &rows, stdout),
        OutputFormat::Json => write_json(&columns, &rows, stdout),
        OutputFormat::Csv => write_delimited(&columns, &rows, stdout, ','),
        OutputFormat::Tsv => write_delimited(&columns, &rows, stdout, '\t'),
        OutputFormat::Table => write_table(&columns, &rows, stdout),
    }
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
        DbValue::Bytes(v) => serde_json::Value::String(format!("\\x{}", hex(v))),
        DbValue::Native { text, .. } => serde_json::Value::String(text.clone()),
    }
}

fn row_object(columns: &[String], row: &[DbValue]) -> serde_json::Map<String, serde_json::Value> {
    columns
        .iter()
        .zip(row)
        .map(|(name, value)| (name.clone(), json_value(value)))
        .collect()
}

fn write_jsonl(
    columns: &[String],
    rows: &[Vec<DbValue>],
    stdout: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    for row in rows {
        serde_json::to_writer(&mut *stdout, &row_object(columns, row))?;
        writeln!(stdout)?;
    }
    Ok(())
}

fn write_json(
    columns: &[String],
    rows: &[Vec<DbValue>],
    stdout: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let objects: Vec<_> = rows.iter().map(|row| row_object(columns, row)).collect();
    serde_json::to_writer(&mut *stdout, &objects)?;
    writeln!(stdout)?;
    Ok(())
}

fn write_delimited(
    columns: &[String],
    rows: &[Vec<DbValue>],
    stdout: &mut dyn std::io::Write,
    sep: char,
) -> std::io::Result<()> {
    writeln!(stdout, "{}", columns.join(&sep.to_string()))?;
    for row in rows {
        let cells: Vec<String> = row.iter().map(display_cell).collect();
        writeln!(stdout, "{}", cells.join(&sep.to_string()))?;
    }
    Ok(())
}

fn write_table(
    columns: &[String],
    rows: &[Vec<DbValue>],
    stdout: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write_delimited(columns, rows, stdout, '|')
}

fn display_cell(value: &DbValue) -> String {
    match value {
        DbValue::Null => String::new(),
        DbValue::Bool(v) => v.to_string(),
        DbValue::I64(v) => v.to_string(),
        DbValue::U64(v) => v.to_string(),
        DbValue::Decimal(v) | DbValue::Text(v) | DbValue::Json(v) => v.clone(),
        DbValue::Bytes(v) => format!("\\x{}", hex(v)),
        DbValue::Native { text, .. } => text.clone(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
