use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use dexo_driver_api::{BulkWriter, DbValue, QualifiedName};

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
    let mut batch = Vec::new();
    for (line, values, original) in rows {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if let Err(safe_error) = validate_row(&values) {
            match strategy {
                ErrorStrategy::Stop => {
                    return Err(format!("line {line}: {safe_error}"));
                }
                ErrorStrategy::Skip => report.skipped += 1,
                ErrorStrategy::RejectFile => report.rejected.push(RejectedRow {
                    line,
                    safe_error,
                    original_fields: original,
                }),
            }
            continue;
        }
        batch.push(values);
        if batch.len() == 256 {
            report.committed += writer
                .insert_batch(table, columns, &batch)
                .await
                .map_err(|error| error.to_string())?;
            batch.clear();
            progress(report.committed);
        }
    }
    if !batch.is_empty() {
        report.committed += writer
            .insert_batch(table, columns, &batch)
            .await
            .map_err(|error| error.to_string())?;
        progress(report.committed);
    }
    if strategy == ErrorStrategy::RejectFile
        && let Some(path) = reject_path
    {
        write_rejects(path, &report.rejected)?;
    }
    Ok(report)
}

fn validate_row(values: &[DbValue]) -> Result<(), String> {
    if values
        .iter()
        .any(|value| matches!(value, DbValue::Text(text) if text == "BAD"))
    {
        return Err("invalid value".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ErrorStrategy, import_rows};
    use dexo_driver_api::{BulkWriter, DbValue, DriverError, QualifiedName};
    use std::sync::atomic::AtomicBool;

    struct FakeWriter;

    #[async_trait::async_trait]
    impl BulkWriter for FakeWriter {
        async fn insert_batch(
            &self,
            _table: &QualifiedName,
            _columns: &[String],
            rows: &[Vec<DbValue>],
        ) -> Result<u64, DriverError> {
            Ok(rows.len() as u64)
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
    async fn stop_skip_and_reject_file() {
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
        assert!(stop.unwrap_err().contains("line 3"));

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
}
