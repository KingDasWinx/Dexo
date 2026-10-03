use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use dexo_driver_api::{BulkWriter, CatalogReader, DbValue, DriverError, QualifiedName};
use tokio::sync::mpsc::Receiver;

use crate::meta_command::{MetaCommand, answer};
use crate::transfer::codec::{Decoded, FormatOptions, TransferFormat, decode_stream};
use crate::transfer::map::{ColumnMapping, map_columns};
use crate::transfer::rejects::{RejectedRow, write_rejects};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorStrategy {
    Stop,
    Skip,
    RejectFile,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    pub committed: u64,
    pub skipped: u64,
    pub rejected: Vec<RejectedRow>,
}

/// Rows written to the database together.
const BATCH: usize = 256;

/// Batches read ahead of the one being written: with it, all an import holds of its file.
const READ_AHEAD: usize = 2;

/// Where an import reads from.
pub enum ImportSource {
    File(PathBuf),
    /// Read once, as it comes: standard input. JSON's columns take a reading of their
    /// own, so a JSON stream is kept in a temporary file to be read twice.
    Stream(Box<dyn Read + Send>),
}

/// What to read, as what, into which table, and what a bad row does.
pub struct ImportRequest {
    pub source: ImportSource,
    pub format: TransferFormat,
    pub target: QualifiedName,
    /// The file's columns renamed or left out; none sends each to its own name.
    pub mapping: Vec<ColumnMapping>,
    pub strategy: ErrorStrategy,
    /// Where `RejectFile` writes the rows it set aside.
    pub reject_path: Option<PathBuf>,
}

/// Reads the file a batch at a time -- never the whole of it -- fits its columns to the
/// table when there is a `catalog` to ask, and writes it. An error after rows went in
/// says how many did; the rows set aside so far are written either way.
pub async fn import_file(
    writer: &dyn BulkWriter,
    catalog: Option<&dyn CatalogReader>,
    request: ImportRequest,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<ImportReport, String> {
    let ImportRequest {
        source,
        format,
        target,
        mapping,
        strategy,
        reject_path,
    } = request;
    let (tx, mut rx) = tokio::sync::mpsc::channel(READ_AHEAD);
    // Not waited for: a reader whose rows are no longer taken stops at its next batch.
    tokio::task::spawn_blocking(move || {
        if let Err(error) = read_source(source, format, &mut |decoded| {
            tx.blocking_send(Ok(decoded)).is_ok()
        }) {
            let _ = tx.blocking_send(Err(error));
        }
    });
    let mut report = ImportReport::default();
    let written: Result<(), String> = async {
        let Some(Decoded::Columns(columns)) = next(&mut rx, cancel).await? else {
            return Ok(());
        };
        let (columns, sources) = map_columns(&columns, &mapping)?;
        let table = target.display_unquoted();
        let (columns, blank_is_null) = match catalog {
            Some(catalog) => {
                fit_columns(&table, &columns, &target_columns(catalog, &table).await?)?
            }
            None => {
                let keep = vec![false; columns.len()];
                (columns, keep)
            }
        };
        while let Some(decoded) = next(&mut rx, cancel).await? {
            let Decoded::Rows(rows) = decoded else {
                continue;
            };
            let rows = rows
                .into_iter()
                .map(|(line, values)| {
                    let mut values: Vec<DbValue> = sources
                        .iter()
                        .map(|&source| values.get(source).cloned().unwrap_or(DbValue::Null))
                        .collect();
                    blank_to_null(&mut values, &blank_is_null);
                    let original = values.iter().map(original_text).collect();
                    (line, values, original)
                })
                .collect();
            let before = report.committed;
            let part = import_rows(
                writer,
                &target,
                &columns,
                rows,
                strategy,
                cancel,
                None,
                |rows| progress(before + rows),
            )
            .await?;
            report.committed += part.committed;
            report.skipped += part.skipped;
            report.rejected.extend(part.rejected);
        }
        Ok(())
    }
    .await;
    if strategy == ErrorStrategy::RejectFile
        && !report.rejected.is_empty()
        && let Some(path) = &reject_path
    {
        write_rejects(path, &report.rejected)?;
    }
    match written {
        Ok(()) => Ok(report),
        Err(error) if error == "cancelled" || report.committed == 0 => Err(error),
        Err(error) => Err(format!(
            "{}. {} went in before it.",
            error.trim_end_matches('.'),
            match report.committed {
                1 => "1 row".to_string(),
                rows => format!("{rows} rows"),
            }
        )),
    }
}

/// The reader's next batch, None at the end; a cancel is seen while it reads.
async fn next(
    rx: &mut Receiver<Result<Decoded, String>>,
    cancel: &AtomicBool,
) -> Result<Option<Decoded>, String> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if let Ok(message) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            return message.transpose();
        }
    }
}

/// Hands the source's columns and rows to `send` a batch at a time, its errors worded
/// for the person importing.
fn read_source(
    source: ImportSource,
    format: TransferFormat,
    send: &mut dyn FnMut(Decoded) -> bool,
) -> Result<(), String> {
    let options = FormatOptions::default();
    let unreadable = |name: &str, error: String| {
        format!(
            "{name} could not be read as {}: {error}. Choose the format that matches the file.",
            format.name()
        )
    };
    let (name, file) = match source {
        ImportSource::File(path) => {
            let file = std::fs::File::open(&path)
                .map_err(|error| format!("{} cannot be read: {error}.", path.display()))?;
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            (name, file)
        }
        ImportSource::Stream(mut stream)
            if matches!(format, TransferFormat::Json | TransferFormat::Jsonl) =>
        {
            let mut file = tempfile::tempfile().map_err(|error| error.to_string())?;
            std::io::copy(&mut stream, &mut file)
                .map_err(|error| format!("The input cannot be read: {error}."))?;
            ("The input".to_string(), file)
        }
        ImportSource::Stream(stream) => {
            let mut stream = Some(stream);
            return decode_stream(
                format,
                &options,
                &mut || -> std::io::Result<Box<dyn Read>> {
                    stream
                        .take()
                        .map(|stream| stream as Box<dyn Read>)
                        .ok_or_else(|| std::io::Error::other("the input is read once"))
                },
                BATCH,
                send,
            )
            .map_err(|error| unreadable("The input", error));
        }
    };
    decode_stream(
        format,
        &options,
        &mut || -> std::io::Result<Box<dyn Read>> {
            let mut file = file.try_clone()?;
            file.seek(SeekFrom::Start(0))?;
            Ok(Box::new(file))
        },
        BATCH,
        send,
    )
    .map_err(|error| unreadable(&name, error))
}

/// A cell as the rejects file shows it.
fn original_text(value: &DbValue) -> String {
    match value {
        DbValue::Null => "NULL".into(),
        DbValue::Bool(value) => value.to_string(),
        DbValue::I64(value) => value.to_string(),
        DbValue::U64(value) => value.to_string(),
        DbValue::Decimal(text) | DbValue::Text(text) | DbValue::Json(text) => text.clone(),
        DbValue::Native { text, .. } => text.clone(),
        DbValue::Bytes(bytes) => format!(
            "\\x{}",
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ),
    }
}

/// Writes `rows` into `table`, each with the number `Line {n}` names it by in a message.
/// With `Stop` the first bad row ends the import, naming its line; `Skip` and
/// `RejectFile` set a bad row aside and carry on, the second writing what was set aside
/// to `reject_path`.
#[allow(clippy::too_many_arguments)]
pub async fn import_rows(
    writer: &dyn BulkWriter,
    table: &QualifiedName,
    columns: &[String],
    rows: Vec<(usize, Vec<DbValue>, Vec<String>)>,
    strategy: ErrorStrategy,
    cancel: &AtomicBool,
    reject_path: Option<&Path>,
    mut progress: impl FnMut(u64),
) -> Result<ImportReport, String> {
    let mut report = ImportReport::default();
    for chunk in rows.chunks(BATCH) {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let batch: Vec<Vec<DbValue>> = chunk.iter().map(|(_, values, _)| values.clone()).collect();
        match writer.insert_batch(table, columns, &batch).await {
            Ok(written) => report.committed += written,
            Err(error) if strategy == ErrorStrategy::Stop => {
                return Err(describe_failure(chunk, &error));
            }
            // The batch is one transaction, undone as a whole: its good rows go in one
            // by one, and the bad ones are set aside.
            Err(_) => {
                for (line, values, original) in chunk {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("cancelled".into());
                    }
                    match writer
                        .insert_batch(table, columns, std::slice::from_ref(values))
                        .await
                    {
                        Ok(written) => report.committed += written,
                        Err(_) if strategy == ErrorStrategy::Skip => report.skipped += 1,
                        Err(error) => report.rejected.push(RejectedRow {
                            line: *line,
                            safe_error: plain_message(&error),
                            original_fields: original.clone(),
                        }),
                    }
                }
            }
        }
        progress(report.committed);
    }
    if strategy == ErrorStrategy::RejectFile
        && !report.rejected.is_empty()
        && let Some(path) = reject_path
    {
        write_rejects(path, &report.rejected)?;
    }
    Ok(report)
}

/// `Line 12: invalid input syntax for type integer: "x"`. The driver says which row of
/// the batch it was; the line is that row's place in the file.
fn describe_failure(chunk: &[(usize, Vec<DbValue>, Vec<String>)], error: &DriverError) -> String {
    let message = plain_message(error);
    let row = error
        .row()
        .and_then(|row| usize::try_from(row).ok())
        .and_then(|row| chunk.get(row.checked_sub(1)?));
    match (row, chunk.first(), chunk.last()) {
        (Some((line, ..)), ..) => format!("Line {line}: {message}"),
        (None, Some((first, ..)), Some((last, ..))) if first != last => {
            format!("Lines {first} to {last}: {message}")
        }
        (None, Some((first, ..)), _) => format!("Line {first}: {message}"),
        _ => message,
    }
}

/// The driver's text without the position MySQL adds -- `at row 1` is the row in the
/// batch, which is not the file's line.
fn plain_message(error: &DriverError) -> String {
    let message = error.to_string();
    match message.rsplit_once(" at row ") {
        Some((head, tail)) if tail.chars().all(|ch| ch.is_ascii_digit()) && !tail.is_empty() => {
            head.to_string()
        }
        _ => message,
    }
}

/// A column of the table an import writes into.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetColumn {
    pub name: String,
    pub type_name: String,
}

/// The columns of `table`, as `\d` lists them. A table that is not there is said so.
pub async fn target_columns(
    catalog: &dyn CatalogReader,
    table: &str,
) -> Result<Vec<TargetColumn>, String> {
    let described = answer(catalog, &MetaCommand::Describe(table.to_string()))
        .await
        .map_err(|error| {
            let message = error.to_string();
            if message.starts_with("Did not find") {
                format!("Table {table} was not found on this connection.")
            } else {
                message
            }
        })?;
    // The rows of `\d` that have a nullable column are the table's columns; the others
    // are its indexes, keys and triggers.
    Ok(described
        .rows
        .into_iter()
        .filter(|row| row.get(2).is_some_and(|nullable| !nullable.is_empty()))
        .map(|row| TargetColumn {
            name: row[0].clone(),
            type_name: row[1].clone(),
        })
        .collect())
}

/// Whether a column keeps an empty cell as the empty text it is. Of the others -- numbers,
/// dates, booleans -- an empty cell can only mean "nothing here".
fn keeps_empty_text(type_name: &str) -> bool {
    let lower = type_name.to_ascii_lowercase();
    ["char", "text", "string", "clob", "name"]
        .iter()
        .any(|word| lower.contains(word))
}

/// The file's columns spelled as the table spells them, and for each whether a blank
/// cell in it is NULL -- in a column that cannot hold empty text. Columns the table does
/// not have are an error naming them. A file written by Dexo's own export reads back as
/// it was: its NULL is `\N`, and its empty strings stay empty in the text columns.
fn fit_columns(
    table: &str,
    columns: &[String],
    target: &[TargetColumn],
) -> Result<(Vec<String>, Vec<bool>), String> {
    let mut fitted = Vec::with_capacity(columns.len());
    let mut unknown = Vec::new();
    let mut blank_is_null = Vec::with_capacity(columns.len());
    for name in columns {
        match target
            .iter()
            .find(|column| column.name.eq_ignore_ascii_case(name))
        {
            Some(column) => {
                fitted.push(column.name.clone());
                blank_is_null.push(!keeps_empty_text(&column.type_name));
            }
            None => unknown.push(name.clone()),
        }
    }
    if !unknown.is_empty() {
        return Err(format!(
            "The file has {} {} that table {table} does not have. The table's columns are: {}.",
            if unknown.len() == 1 {
                "a column"
            } else {
                "columns"
            },
            unknown.join(", "),
            target
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok((fitted, blank_is_null))
}

fn blank_to_null(values: &mut [DbValue], blank_is_null: &[bool]) {
    for (value, null) in values.iter_mut().zip(blank_is_null) {
        if *null && matches!(value, DbValue::Text(text) if text.is_empty()) {
            *value = DbValue::Null;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ErrorStrategy, ImportRequest, ImportSource, TargetColumn, blank_to_null, fit_columns,
        import_file, import_rows,
    };
    use crate::transfer::codec::TransferFormat;
    use dexo_driver_api::{BulkWriter, DbValue, DriverError, DriverErrorCategory, QualifiedName};
    use std::sync::atomic::AtomicBool;

    /// A table that refuses the value `BAD`, as a database refuses a text in a number
    /// column: the batch with it fails as a whole, and says which row.
    struct FakeWriter;

    #[async_trait::async_trait]
    impl BulkWriter for FakeWriter {
        async fn insert_batch(
            &self,
            _table: &QualifiedName,
            _columns: &[String],
            rows: &[Vec<DbValue>],
        ) -> Result<u64, DriverError> {
            match rows
                .iter()
                .position(|row| row.contains(&DbValue::Text("BAD".into())))
            {
                Some(index) => Err(DriverError::new(
                    DriverErrorCategory::Syntax,
                    "Incorrect integer value: 'BAD' for column 'qty' at row 1",
                )
                .with_row(index as u32 + 1)),
                None => Ok(rows.len() as u64),
            }
        }
    }

    fn table() -> QualifiedName {
        QualifiedName::new(None::<String>, None::<String>, "t")
    }

    fn rows() -> Vec<(usize, Vec<DbValue>, Vec<String>)> {
        vec![
            (2, vec![DbValue::Text("ok".into())], vec!["ok".into()]),
            (3, vec![DbValue::Text("BAD".into())], vec!["BAD".into()]),
            (4, vec![DbValue::Text("ok2".into())], vec!["ok2".into()]),
        ]
    }

    #[tokio::test]
    async fn stop_names_the_line_and_skip_and_reject_file_carry_on() {
        let cancel = AtomicBool::new(false);
        let stop = import_rows(
            &FakeWriter,
            &table(),
            &["a".into()],
            rows(),
            ErrorStrategy::Stop,
            &cancel,
            None,
            |_| {},
        )
        .await;
        // The line of the file, not the `at row 1` the server counted in the batch.
        assert_eq!(
            stop.unwrap_err(),
            "Line 3: Incorrect integer value: 'BAD' for column 'qty'"
        );

        let skipped = import_rows(
            &FakeWriter,
            &table(),
            &["a".into()],
            rows(),
            ErrorStrategy::Skip,
            &cancel,
            None,
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(skipped.committed, 2);
        assert_eq!(skipped.skipped, 1);

        let dir = tempfile::tempdir().unwrap();
        let reject = dir.path().join("rejects.csv");
        let rejected = import_rows(
            &FakeWriter,
            &table(),
            &["a".into()],
            rows(),
            ErrorStrategy::RejectFile,
            &cancel,
            Some(&reject),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(rejected.committed, 2);
        assert_eq!(rejected.rejected.len(), 1);
        let body = std::fs::read_to_string(&reject).unwrap();
        assert!(body.contains("BAD"));
        assert!(body.contains("line,error"));
    }

    fn target() -> Vec<TargetColumn> {
        [
            ("id", "integer"),
            ("name", "character varying"),
            ("qty", "integer"),
        ]
        .iter()
        .map(|(name, type_name)| TargetColumn {
            name: name.to_string(),
            type_name: type_name.to_string(),
        })
        .collect()
    }

    #[test]
    fn a_blank_cell_is_null_unless_the_column_holds_text() {
        let (columns, blank_is_null) = fit_columns(
            "tbl",
            &["ID".into(), "name".into(), "qty".into()],
            &target(),
        )
        .unwrap();
        // The table's spelling, an empty name kept, an empty number nothing.
        assert_eq!(columns, ["id", "name", "qty"]);
        let mut row = vec![
            DbValue::Text("1".into()),
            DbValue::Text(String::new()),
            DbValue::Text(String::new()),
        ];
        blank_to_null(&mut row, &blank_is_null);
        assert_eq!(
            row,
            vec![
                DbValue::Text("1".into()),
                DbValue::Text(String::new()),
                DbValue::Null
            ]
        );
    }

    /// The header line read as one column name was the whole of Dexo's TSV import
    /// failure; the message now says what the file has that the table does not.
    #[test]
    fn columns_the_table_does_not_have_are_named() {
        let error = fit_columns("tbl", &["idnameqty".into()], &target()).unwrap_err();
        assert_eq!(
            error,
            "The file has a column idnameqty that table tbl does not have. The table's columns are: id, name, qty."
        );
    }

    /// A batch as a writer was given it: its columns and its rows.
    type Written = (Vec<String>, Vec<Vec<DbValue>>);

    /// Takes what FakeWriter takes, and keeps each batch it was given.
    #[derive(Default)]
    struct Recorder {
        batches: std::sync::Mutex<Vec<Written>>,
    }

    #[async_trait::async_trait]
    impl BulkWriter for Recorder {
        async fn insert_batch(
            &self,
            table: &QualifiedName,
            columns: &[String],
            rows: &[Vec<DbValue>],
        ) -> Result<u64, DriverError> {
            let written = FakeWriter.insert_batch(table, columns, rows).await?;
            self.batches
                .lock()
                .unwrap()
                .push((columns.to_vec(), rows.to_vec()));
            Ok(written)
        }
    }

    fn request(source: ImportSource, format: TransferFormat) -> ImportRequest {
        ImportRequest {
            source,
            format,
            target: table(),
            mapping: Vec::new(),
            strategy: ErrorStrategy::Stop,
            reject_path: None,
        }
    }

    fn csv_file(dir: &tempfile::TempDir, rows: usize, bad: Option<usize>) -> std::path::PathBuf {
        let mut body = String::from("qty\n");
        for row in 1..=rows {
            if bad == Some(row) {
                body.push_str("BAD\n");
            } else {
                body.push_str(&format!("{row}\n"));
            }
        }
        let path = dir.path().join("rows.csv");
        std::fs::write(&path, body).unwrap();
        path
    }

    /// A file goes in a batch at a time, in its order, never held whole.
    #[tokio::test]
    async fn a_file_goes_in_a_batch_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = csv_file(&dir, 600, None);
        let recorder = Recorder::default();
        let mut seen = 0;
        let report = import_file(
            &recorder,
            None,
            request(ImportSource::File(path), TransferFormat::Csv),
            &AtomicBool::new(false),
            |rows| seen = rows,
        )
        .await
        .unwrap();
        assert_eq!(report.committed, 600);
        assert_eq!(seen, 600);
        let batches = recorder.batches.lock().unwrap();
        assert_eq!(
            batches
                .iter()
                .map(|(_, rows)| rows.len())
                .collect::<Vec<_>>(),
            [256, 256, 88]
        );
        assert_eq!(
            batches[2].1.last(),
            Some(&vec![DbValue::Text("600".into())])
        );
    }

    /// A bad row stops the import on its own line, and says what went in before it.
    #[tokio::test]
    async fn a_stop_names_the_line_and_what_went_in_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = csv_file(&dir, 600, Some(300));
        let error = import_file(
            &FakeWriter,
            None,
            request(ImportSource::File(path), TransferFormat::Csv),
            &AtomicBool::new(false),
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            "Line 301: Incorrect integer value: 'BAD' for column 'qty'. 256 rows went in before it."
        );
    }

    /// JSON from a stream is kept to be read twice: its columns are every object's keys.
    /// A mapping renames and leaves out by name; set-aside rows go to the rejects file.
    #[tokio::test]
    async fn a_json_stream_gets_every_key_the_mapping_and_its_rejects() {
        let dir = tempfile::tempdir().unwrap();
        let rejects = dir.path().join("in.rejects.csv");
        let input = "{\"a\":1,\"b\":2}\n\n{\"a\":\"BAD\",\"c\":3}\n{\"c\":4}\n";
        let recorder = Recorder::default();
        let report = import_file(
            &recorder,
            None,
            ImportRequest {
                mapping: crate::transfer::map::parse_mapping(&["b=".into(), "c=see".into()])
                    .unwrap(),
                strategy: ErrorStrategy::RejectFile,
                reject_path: Some(rejects.clone()),
                ..request(
                    ImportSource::Stream(Box::new(std::io::Cursor::new(input.as_bytes()))),
                    TransferFormat::Jsonl,
                )
            },
            &AtomicBool::new(false),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(report.committed, 2);
        assert_eq!(report.rejected.len(), 1);
        // The bad object is on the file's third line, the blank one counted.
        assert_eq!(report.rejected[0].line, 3);
        assert!(std::fs::read_to_string(&rejects).unwrap().contains("BAD"));
        let batches = recorder.batches.lock().unwrap();
        assert_eq!(batches[0].0, ["a", "see"]);
        assert_eq!(batches[0].1[0], vec![DbValue::I64(1), DbValue::Null]);
    }

    /// A file that is not what it was said to be is named, with the format to choose.
    #[tokio::test]
    async fn a_file_in_another_format_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rows.json");
        std::fs::write(&path, "qty\n1\n").unwrap();
        let error = import_file(
            &FakeWriter,
            None,
            request(ImportSource::File(path), TransferFormat::Json),
            &AtomicBool::new(false),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(
            error.starts_with("rows.json could not be read as JSON: ")
                && error.ends_with("Choose the format that matches the file."),
            "{error}"
        );
    }
}
