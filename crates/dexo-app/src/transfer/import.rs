use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use dexo_driver_api::{BulkWriter, CatalogReader, DbValue, DriverError, QualifiedName};

use crate::meta_command::{MetaCommand, answer};
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

/// What a file's rows need to go into `target`: the columns spelled as the table spells
/// them, an error naming the columns it does not have, and an empty cell in a column that
/// cannot hold empty text as NULL. A file written by Dexo's own export reads back as it
/// was: its NULL is `\N`, and its empty strings stay empty in the text columns.
pub fn fit_to_table(
    table: &str,
    columns: Vec<String>,
    mut rows: Vec<Vec<DbValue>>,
    target: &[TargetColumn],
) -> Result<(Vec<String>, Vec<Vec<DbValue>>), String> {
    let mut fitted = Vec::with_capacity(columns.len());
    let mut unknown = Vec::new();
    let mut blank_is_null = Vec::with_capacity(columns.len());
    for name in &columns {
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
    for row in &mut rows {
        for (value, null) in row.iter_mut().zip(&blank_is_null) {
            if *null && matches!(value, DbValue::Text(text) if text.is_empty()) {
                *value = DbValue::Null;
            }
        }
    }
    Ok((fitted, rows))
}

#[cfg(test)]
mod tests {
    use super::{ErrorStrategy, TargetColumn, fit_to_table, import_rows};
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
        let (columns, rows) = fit_to_table(
            "tbl",
            vec!["ID".into(), "name".into(), "qty".into()],
            vec![vec![
                DbValue::Text("1".into()),
                DbValue::Text(String::new()),
                DbValue::Text(String::new()),
            ]],
            &target(),
        )
        .unwrap();
        // The table's spelling, an empty name kept, an empty number nothing.
        assert_eq!(columns, ["id", "name", "qty"]);
        assert_eq!(
            rows[0],
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
        let error =
            fit_to_table("tbl", vec!["idnameqty".into()], Vec::new(), &target()).unwrap_err();
        assert_eq!(
            error,
            "The file has a column idnameqty that table tbl does not have. The table's columns are: id, name, qty."
        );
    }
}
