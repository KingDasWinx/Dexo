use dexo_driver_api::DbValue;

use crate::data::copy::{SqlDialect, sql_literal};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferFormat {
    Csv,
    Tsv,
    Json,
    Jsonl,
    Sql,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryMode {
    Hex,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormatOptions {
    pub null: String,
    pub delimiter: u8,
    pub header: bool,
    pub encoding: &'static encoding_rs::Encoding,
    pub binary: BinaryMode,
    pub dialect: SqlDialect,
    /// The table an SQL export inserts into, unquoted, `schema.table` or `table`.
    /// `export_rows` names it after the file when it is not given.
    pub table: Option<String>,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            null: "\\N".into(),
            delimiter: b',',
            header: true,
            encoding: encoding_rs::UTF_8,
            binary: BinaryMode::Hex,
            dialect: SqlDialect::Postgres,
            table: None,
        }
    }
}

pub fn encode_document(
    format: TransferFormat,
    options: &FormatOptions,
    columns: &[String],
    rows: &[Vec<DbValue>],
) -> Result<Vec<u8>, String> {
    let mut sink = Vec::new();
    let mut encoder = StreamEncoder::new(&mut sink, format, options, columns)?;
    for row in rows {
        encoder.write_row(row)?;
    }
    encoder.finish()?;
    Ok(sink)
}

pub fn decode_document(
    format: TransferFormat,
    options: &FormatOptions,
    bytes: &[u8],
) -> Result<(Vec<String>, Vec<Vec<DbValue>>), String> {
    let text = decode_text(options.encoding, bytes)?;
    match format {
        TransferFormat::Csv => decode_delimited(&text, options.delimiter, options),
        // The format says the delimiter, as it does when writing: a caller that forgot
        // to set one read a whole header line as a single column.
        TransferFormat::Tsv => decode_delimited(&text, b'\t', options),
        TransferFormat::Json => decode_json(&text, true),
        TransferFormat::Jsonl => decode_jsonl(&text),
        TransferFormat::Sql => Err(
            "an SQL file is a script, not data to import: run it with `dexo run --file`, or import CSV or JSON"
                .into(),
        ),
    }
}

pub struct StreamEncoder<'a, W: std::io::Write> {
    writer: &'a mut W,
    format: TransferFormat,
    options: &'a FormatOptions,
    columns: &'a [String],
    json_rows: usize,
    finished: bool,
    /// Bytes handed to the writer so far.
    written: u64,
}

impl<'a, W: std::io::Write> StreamEncoder<'a, W> {
    pub fn new(
        writer: &'a mut W,
        format: TransferFormat,
        options: &'a FormatOptions,
        columns: &'a [String],
    ) -> Result<Self, String> {
        let mut encoder = Self {
            writer,
            format,
            options,
            columns,
            json_rows: 0,
            finished: false,
            written: 0,
        };
        encoder.start()?;
        Ok(encoder)
    }

    fn put(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writer
            .write_all(bytes)
            .map_err(|error| error.to_string())?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    fn start(&mut self) -> Result<(), String> {
        match self.format {
            TransferFormat::Csv | TransferFormat::Tsv if self.options.header => {
                self.write_delimited(self.columns.iter().map(String::as_str))?;
            }
            TransferFormat::Json => self.put(b"[")?,
            _ => {}
        }
        Ok(())
    }

    /// Writes one row and returns the bytes it took.
    pub fn write_row(&mut self, row: &[DbValue]) -> Result<u64, String> {
        let before = self.written;
        match self.format {
            TransferFormat::Csv | TransferFormat::Tsv => {
                let fields: Vec<String> =
                    row.iter().map(|value| field(value, self.options)).collect();
                self.write_delimited(fields.iter().map(String::as_str))?;
            }
            // One object a line, so the file reads and diffs: `[`, the objects with a
            // comma after each but the last, `]`.
            TransferFormat::Json => {
                self.put(if self.json_rows > 0 { b",\n" } else { b"\n" })?;
                let object = json_object(self.columns, row);
                self.put(object.as_bytes())?;
                self.json_rows += 1;
            }
            TransferFormat::Jsonl => {
                let object = json_object(self.columns, row);
                self.put(object.as_bytes())?;
                self.put(b"\n")?;
            }
            TransferFormat::Sql => {
                let table = self.options.table.as_deref().unwrap_or("dest");
                let sql = sql_insert(table, self.columns, row, self.options.dialect);
                self.put(sql.as_bytes())?;
                self.put(b"\n")?;
            }
        }
        Ok(self.written - before)
    }

    fn write_delimited<'b>(&mut self, fields: impl Iterator<Item = &'b str>) -> Result<(), String> {
        let delimiter = match self.format {
            TransferFormat::Tsv => b'\t',
            _ => self.options.delimiter,
        };
        let mut writer = csv::WriterBuilder::new()
            .delimiter(delimiter)
            .from_writer(Vec::new());
        writer
            .write_record(fields)
            .map_err(|error| error.to_string())?;
        let bytes = writer.into_inner().map_err(|error| error.to_string())?;
        self.put(&bytes)
    }

    /// Bytes written so far: the header and the brackets of a JSON array too.
    pub fn bytes_written(&self) -> u64 {
        self.written
    }

    /// Ends the document and returns the bytes it came to.
    pub fn finish(mut self) -> Result<u64, String> {
        if self.format == TransferFormat::Json {
            self.put(if self.json_rows > 0 { b"\n]\n" } else { b"]\n" })?;
        }
        self.writer.flush().map_err(|error| error.to_string())?;
        self.finished = true;
        Ok(self.written)
    }
}

fn field(value: &DbValue, options: &FormatOptions) -> String {
    match value {
        DbValue::Null => options.null.clone(),
        DbValue::Bytes(bytes) => format!("\\x{}", hex(bytes)),
        other => display(other),
    }
}

fn display(value: &DbValue) -> String {
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

/// One row as a JSON object with its keys in the columns' order: `serde_json`'s map sorts
/// them, which put `id` after `b`.
fn json_object(columns: &[String], row: &[DbValue]) -> String {
    let mut out = String::from("{");
    for (index, (name, value)) in columns.iter().zip(row.iter()).enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&serde_json::Value::String(name.clone()).to_string());
        out.push(':');
        out.push_str(&json_value(value));
    }
    out.push('}');
    out
}

/// A value as JSON text. A number stays a number and a JSON document stays a document,
/// as they would read back; anything the format cannot hold as one is a string.
fn json_value(value: &DbValue) -> String {
    match value {
        DbValue::Null => "null".into(),
        DbValue::Bool(v) => v.to_string(),
        DbValue::I64(v) => v.to_string(),
        DbValue::U64(v) => v.to_string(),
        DbValue::Decimal(v) if is_json_number(v) => v.clone(),
        DbValue::Json(v) => match serde_json::from_str::<serde_json::Value>(v) {
            // A line of JSON Lines is one line: a document with line breaks is written
            // compact.
            Ok(document) if v.contains(['\n', '\r']) => document.to_string(),
            Ok(_) => v.clone(),
            Err(_) => serde_json::Value::String(v.clone()).to_string(),
        },
        DbValue::Decimal(v) | DbValue::Text(v) | DbValue::Native { text: v, .. } => {
            serde_json::Value::String(v.clone()).to_string()
        }
        DbValue::Bytes(v) => serde_json::json!({ "$hex": hex(v) }).to_string(),
    }
}

/// Whether `text` is a number as JSON writes one: no `+`, no `.5`, no `NaN`.
fn is_json_number(text: &str) -> bool {
    serde_json::from_str::<serde_json::Number>(text).is_ok()
}

pub(crate) fn sql_insert(
    table: &str,
    columns: &[String],
    row: &[DbValue],
    dialect: SqlDialect,
) -> String {
    let ident = |name: &str| match dialect {
        SqlDialect::Postgres | SqlDialect::Sqlite | SqlDialect::Duckdb => {
            format!("\"{}\"", name.replace('"', "\"\""))
        }
        SqlDialect::Mysql => format!("`{}`", name.replace('`', "``")),
    };
    let table = table.split('.').map(ident).collect::<Vec<_>>().join(".");
    let cols = columns
        .iter()
        .map(|name| ident(name))
        .collect::<Vec<_>>()
        .join(", ");
    let values = row
        .iter()
        .map(|value| sql_literal(value, dialect))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO {table} ({cols}) VALUES ({values});")
}

fn decode_text(encoding: &'static encoding_rs::Encoding, bytes: &[u8]) -> Result<String, String> {
    let (text, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        return Err("input is not valid in the declared encoding".into());
    }
    Ok(text.into_owned())
}

fn decode_delimited(
    text: &str,
    delimiter: u8,
    options: &FormatOptions,
) -> Result<(Vec<String>, Vec<Vec<DbValue>>), String> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(options.header)
        .from_reader(text.as_bytes());
    let columns = if options.header {
        reader
            .headers()
            .map_err(|error| error.to_string())?
            .iter()
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let record = record.map_err(|error| format!("line {}: {error}", index + 2))?;
        let values = record
            .iter()
            .map(|field| parse_field(field, options))
            .collect();
        rows.push(values);
    }
    let columns = if columns.is_empty() {
        (0..rows.first().map(Vec::len).unwrap_or(0))
            .map(|i| format!("c{i}"))
            .collect()
    } else {
        columns
    };
    Ok((columns, rows))
}

fn parse_field(field: &str, options: &FormatOptions) -> DbValue {
    if field == options.null {
        DbValue::Null
    } else if let Some(hex) = field.strip_prefix("\\x") {
        DbValue::Bytes(decode_hex(hex))
    } else {
        DbValue::Text(field.to_string())
    }
}

fn decode_hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

fn decode_json(text: &str, array: bool) -> Result<(Vec<String>, Vec<Vec<DbValue>>), String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let rows = if array {
        value
            .as_array()
            .ok_or_else(|| "JSON export must be an array".to_string())?
            .clone()
    } else {
        vec![value]
    };
    json_rows(rows)
}

fn decode_jsonl(text: &str) -> Result<(Vec<String>, Vec<Vec<DbValue>>), String> {
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|error| format!("line {}: {error}", index + 1))?;
        rows.push(value);
    }
    json_rows(rows)
}

fn json_rows(objects: Vec<serde_json::Value>) -> Result<(Vec<String>, Vec<Vec<DbValue>>), String> {
    let mut columns = Vec::new();
    for object in &objects {
        if let Some(map) = object.as_object() {
            for key in map.keys() {
                if !columns.contains(key) {
                    columns.push(key.clone());
                }
            }
        }
    }
    let mut rows = Vec::new();
    for object in objects {
        let map = object
            .as_object()
            .ok_or_else(|| "JSON row must be an object".to_string())?;
        rows.push(
            columns
                .iter()
                .map(|column| json_to_value(map.get(column).unwrap_or(&serde_json::Value::Null)))
                .collect(),
        );
    }
    Ok((columns, rows))
}

fn json_to_value(value: &serde_json::Value) -> DbValue {
    match value {
        serde_json::Value::Null => DbValue::Null,
        serde_json::Value::Bool(v) => DbValue::Bool(*v),
        serde_json::Value::Number(v) => v
            .as_i64()
            .map(DbValue::I64)
            .or_else(|| v.as_u64().map(DbValue::U64))
            .unwrap_or_else(|| DbValue::Decimal(v.to_string())),
        serde_json::Value::String(v) => DbValue::Text(v.clone()),
        serde_json::Value::Object(map) => {
            if let Some(hex) = map.get("$hex").and_then(|value| value.as_str()) {
                DbValue::Bytes(decode_hex(hex))
            } else {
                DbValue::Json(value.to_string())
            }
        }
        other => DbValue::Json(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{FormatOptions, TransferFormat, decode_document, encode_document};
    use dexo_driver_api::DbValue;

    fn sample() -> (Vec<String>, Vec<Vec<DbValue>>) {
        (
            vec!["a".into(), "b".into(), "c".into()],
            vec![
                vec![
                    DbValue::Null,
                    DbValue::Text(String::new()),
                    DbValue::Text("say \"hi\"".into()),
                ],
                vec![
                    DbValue::Text("line\nbreak".into()),
                    DbValue::Text("café".into()),
                    DbValue::Decimal("1.50".into()),
                ],
                vec![
                    DbValue::Text("2026-08-14".into()),
                    DbValue::Bytes(vec![0xde, 0xad]),
                    DbValue::I64(3),
                ],
            ],
        )
    }

    #[test]
    fn codecs_round_trip_lossless_formats() {
        let (columns, rows) = sample();
        let mut options = FormatOptions::default();
        for format in [
            TransferFormat::Csv,
            TransferFormat::Tsv,
            TransferFormat::Json,
            TransferFormat::Jsonl,
        ] {
            if format == TransferFormat::Tsv {
                options.delimiter = b'\t';
            } else {
                options.delimiter = b',';
            }
            let encoded = encode_document(format, &options, &columns, &rows).unwrap();
            let (back_cols, back_rows) = decode_document(format, &options, &encoded).unwrap();
            assert_eq!(back_cols, columns);
            assert_eq!(back_rows.len(), rows.len());
            assert!(matches!(back_rows[0][0], DbValue::Null));
            assert_eq!(back_rows[0][1], DbValue::Text(String::new()));
            if format != TransferFormat::Json {
                assert!(encoded.contains(&b'\n') || encoded.contains(&b'\r'));
            }
        }
        let sql = encode_document(TransferFormat::Sql, &options, &columns, &rows).unwrap();
        let sql = String::from_utf8(sql).unwrap();
        assert!(sql.contains("NULL"));
        assert!(sql.contains("'café'"));
        assert!(decode_document(TransferFormat::Sql, &options, sql.as_bytes()).is_err());
    }

    /// Dexo reads its own TSV back with the options it is given by default: the format,
    /// not the caller, says the delimiter.
    #[test]
    fn a_tsv_is_read_with_tabs_by_default() {
        let (columns, rows) = sample();
        let options = FormatOptions::default();
        let encoded = encode_document(TransferFormat::Tsv, &options, &columns, &rows).unwrap();
        let (back_columns, back_rows) =
            decode_document(TransferFormat::Tsv, &options, &encoded).unwrap();
        assert_eq!(back_columns, columns);
        assert_eq!(back_rows.len(), rows.len());
    }

    /// A JSON export keeps the columns' order, numbers as numbers and JSON documents as
    /// documents, one object to a line, with a newline at the end.
    #[test]
    fn json_export_reads_like_the_result() {
        let columns: Vec<String> = ["id", "txt", "j", "num", "b"]
            .iter()
            .map(|name| name.to_string())
            .collect();
        let rows = vec![vec![
            DbValue::I64(1),
            DbValue::Text("a,b".into()),
            DbValue::Json("{\"a\": 1}".into()),
            DbValue::Decimal("1.50".into()),
            DbValue::Bool(true),
        ]];
        let options = FormatOptions::default();
        let json = encode_document(TransferFormat::Json, &options, &columns, &rows).unwrap();
        assert_eq!(
            String::from_utf8(json).unwrap(),
            "[\n{\"id\":1,\"txt\":\"a,b\",\"j\":{\"a\": 1},\"num\":1.50,\"b\":true}\n]\n"
        );
        let jsonl = encode_document(TransferFormat::Jsonl, &options, &columns, &rows).unwrap();
        assert!(String::from_utf8(jsonl).unwrap().starts_with("{\"id\":1,"));
        let empty = encode_document(TransferFormat::Json, &options, &columns, &[]).unwrap();
        assert_eq!(empty, b"[]\n");
        // A document with line breaks must not break its line.
        let broken = vec![vec![
            DbValue::I64(1),
            DbValue::Text(String::new()),
            DbValue::Json("{\n \"a\": 1\n}".into()),
            DbValue::Null,
            DbValue::Null,
        ]];
        let jsonl = encode_document(TransferFormat::Jsonl, &options, &columns, &broken).unwrap();
        assert_eq!(String::from_utf8(jsonl).unwrap().lines().count(), 1);
    }

    /// The count of bytes the progress shows is bytes, not rows.
    #[test]
    fn a_row_reports_the_bytes_it_took() {
        let mut sink = Vec::new();
        let options = FormatOptions::default();
        let columns = vec!["a".to_string()];
        let mut encoder =
            super::StreamEncoder::new(&mut sink, TransferFormat::Csv, &options, &columns).unwrap();
        let bytes = encoder.write_row(&[DbValue::Text("hello".into())]).unwrap();
        assert_eq!(bytes, "hello\n".len() as u64);
    }

    #[test]
    fn json_number_to_decimal_is_documented_lossy() {
        let json = br#"[{"n":1.50}]"#;
        let (_, rows) =
            decode_document(TransferFormat::Json, &FormatOptions::default(), json).unwrap();
        assert!(matches!(rows[0][0], DbValue::Decimal(_) | DbValue::I64(_)));
    }
}
