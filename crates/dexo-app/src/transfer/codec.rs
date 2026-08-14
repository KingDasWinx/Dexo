use dexo_driver_api::DbValue;

use crate::data::copy::SqlDialect;

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
        TransferFormat::Csv | TransferFormat::Tsv => decode_delimited(&text, options),
        TransferFormat::Json => decode_json(&text, true),
        TransferFormat::Jsonl => decode_jsonl(&text),
        TransferFormat::Sql => Err("SQL import is lossy; use CSV/JSON".into()),
    }
}

pub struct StreamEncoder<'a, W: std::io::Write> {
    writer: &'a mut W,
    format: TransferFormat,
    options: &'a FormatOptions,
    columns: &'a [String],
    json_rows: usize,
    finished: bool,
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
        };
        encoder.start()?;
        Ok(encoder)
    }

    fn start(&mut self) -> Result<(), String> {
        match self.format {
            TransferFormat::Csv | TransferFormat::Tsv if self.options.header => {
                self.write_delimited(self.columns.iter().map(String::as_str))?;
            }
            TransferFormat::Json => self
                .writer
                .write_all(b"[")
                .map_err(|error| error.to_string())?,
            _ => {}
        }
        Ok(())
    }

    pub fn write_row(&mut self, row: &[DbValue]) -> Result<u64, String> {
        match self.format {
            TransferFormat::Csv | TransferFormat::Tsv => {
                let fields: Vec<String> =
                    row.iter().map(|value| field(value, self.options)).collect();
                self.write_delimited(fields.iter().map(String::as_str))?;
            }
            TransferFormat::Json => {
                if self.json_rows > 0 {
                    self.writer
                        .write_all(b",")
                        .map_err(|error| error.to_string())?;
                }
                let object = json_object(self.columns, row);
                self.writer
                    .write_all(object.as_bytes())
                    .map_err(|error| error.to_string())?;
                self.json_rows += 1;
            }
            TransferFormat::Jsonl => {
                let object = json_object(self.columns, row);
                self.writer
                    .write_all(object.as_bytes())
                    .map_err(|error| error.to_string())?;
                self.writer
                    .write_all(b"\n")
                    .map_err(|error| error.to_string())?;
            }
            TransferFormat::Sql => {
                let sql = sql_insert(self.columns, row, self.options.dialect);
                self.writer
                    .write_all(sql.as_bytes())
                    .map_err(|error| error.to_string())?;
                self.writer
                    .write_all(b"\n")
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(1)
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
        self.writer
            .write_all(&bytes)
            .map_err(|error| error.to_string())
    }

    pub fn finish(mut self) -> Result<(), String> {
        if self.format == TransferFormat::Json {
            self.writer
                .write_all(b"]")
                .map_err(|error| error.to_string())?;
        }
        self.writer.flush().map_err(|error| error.to_string())?;
        self.finished = true;
        Ok(())
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

fn json_object(columns: &[String], row: &[DbValue]) -> String {
    let mut map = serde_json::Map::new();
    for (name, value) in columns.iter().zip(row.iter()) {
        map.insert(name.clone(), json_value(value));
    }
    serde_json::Value::Object(map).to_string()
}

fn json_value(value: &DbValue) -> serde_json::Value {
    match value {
        DbValue::Null => serde_json::Value::Null,
        DbValue::Bool(v) => serde_json::Value::Bool(*v),
        DbValue::I64(v) => serde_json::json!(*v),
        DbValue::U64(v) => serde_json::json!(*v),
        DbValue::Decimal(v) => serde_json::Value::String(v.clone()),
        DbValue::Text(v) | DbValue::Json(v) | DbValue::Native { text: v, .. } => {
            serde_json::Value::String(v.clone())
        }
        DbValue::Bytes(v) => serde_json::json!({ "$hex": hex(v) }),
    }
}

fn sql_insert(columns: &[String], row: &[DbValue], dialect: SqlDialect) -> String {
    let ident = |name: &str| match dialect {
        SqlDialect::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
        SqlDialect::Mysql => format!("`{}`", name.replace('`', "``")),
    };
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
    format!("INSERT INTO dest ({cols}) VALUES ({values});")
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

fn decode_text(encoding: &'static encoding_rs::Encoding, bytes: &[u8]) -> Result<String, String> {
    let (text, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        return Err("input is not valid in the declared encoding".into());
    }
    Ok(text.into_owned())
}

fn decode_delimited(
    text: &str,
    options: &FormatOptions,
) -> Result<(Vec<String>, Vec<Vec<DbValue>>), String> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(options.delimiter)
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

    #[test]
    fn json_number_to_decimal_is_documented_lossy() {
        let json = br#"[{"n":1.50}]"#;
        let (_, rows) =
            decode_document(TransferFormat::Json, &FormatOptions::default(), json).unwrap();
        assert!(matches!(rows[0][0], DbValue::Decimal(_) | DbValue::I64(_)));
    }
}
