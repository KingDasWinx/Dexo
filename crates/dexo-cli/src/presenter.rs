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
            QueryEvent::Finished { .. }
            | QueryEvent::ResultSetStarted { .. }
            | QueryEvent::ResultSetFinished { .. } => {}
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

/// CSV and TSV as `dexo export` writes them: values quoted where they hold the
/// separator, a quote or a line break, and NULL as `\N`, apart from an empty string.
fn write_delimited(
    columns: &[String],
    rows: &[Vec<DbValue>],
    stdout: &mut dyn std::io::Write,
    sep: char,
) -> std::io::Result<()> {
    use dexo_app::transfer::{FormatOptions, TransferFormat, encode_document};
    let format = if sep == '\t' {
        TransferFormat::Tsv
    } else {
        TransferFormat::Csv
    };
    let bytes = encode_document(format, &FormatOptions::default(), columns, rows)
        .map_err(std::io::Error::other)?;
    stdout.write_all(&bytes)
}

/// Columns padded to their widest value, a rule under the names, NULL as `<null>` so it
/// reads apart from an empty string, and a value's line breaks as spaces so a row stays
/// on one line.
fn write_table(
    columns: &[String],
    rows: &[Vec<DbValue>],
    stdout: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row.iter().map(table_cell).collect())
        .collect();
    let widths: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(index, name)| {
            cells
                .iter()
                .filter_map(|row| row.get(index))
                .map(|cell| cell.chars().count())
                .chain([name.chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |values: &mut dyn Iterator<Item = &str>| {
        values
            .zip(&widths)
            .map(|(value, width)| format!("{value:<width$}"))
            .collect::<Vec<_>>()
            .join(" | ")
            .trim_end()
            .to_string()
    };
    writeln!(stdout, "{}", line(&mut columns.iter().map(String::as_str)))?;
    writeln!(
        stdout,
        "{}",
        widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join("-+-")
    )?;
    for row in &cells {
        writeln!(stdout, "{}", line(&mut row.iter().map(String::as_str)))?;
    }
    Ok(())
}

fn table_cell(value: &DbValue) -> String {
    match value {
        DbValue::Null => "<null>".into(),
        DbValue::Bool(v) => v.to_string(),
        DbValue::I64(v) => v.to_string(),
        DbValue::U64(v) => v.to_string(),
        DbValue::Decimal(v) | DbValue::Text(v) | DbValue::Json(v) => {
            v.replace(['\n', '\r', '\t'], " ")
        }
        DbValue::Bytes(v) => format!("\\x{}", hex(v)),
        DbValue::Native { text, .. } => text.replace(['\n', '\r', '\t'], " "),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
