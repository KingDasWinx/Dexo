use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use dexo_driver_api::DbValue;
use tempfile::NamedTempFile;

use crate::transfer::codec::{FormatOptions, StreamEncoder, TransferFormat};

#[derive(Clone, Debug, PartialEq)]
pub struct ExportProgress {
    pub rows: u64,
    pub bytes: u64,
}

#[derive(Clone, Default)]
pub struct RecordingSink {
    max_held: Arc<AtomicUsize>,
    written: Arc<AtomicUsize>,
}

impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_rows_held(&self) -> usize {
        self.max_held.load(Ordering::SeqCst)
    }

    pub fn rows_written(&self) -> usize {
        self.written.load(Ordering::SeqCst)
    }
}

pub async fn export_row_batches(
    batches: impl IntoIterator<Item = Vec<Vec<DbValue>>>,
    sink: RecordingSink,
) -> Result<(), ExportError> {
    for batch in batches {
        sink.max_held.fetch_max(batch.len(), Ordering::SeqCst);
        sink.written.fetch_add(batch.len(), Ordering::SeqCst);
    }
    Ok(())
}

/// An error writing into `folder`, in words: the OS's own text names no folder.
fn describe_io(error: &std::io::Error, folder: &Path) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            format!("The folder {} does not exist.", folder.display())
        }
        std::io::ErrorKind::PermissionDenied => {
            format!("Dexo may not write in {}.", folder.display())
        }
        _ => error.to_string(),
    }
}

#[derive(Debug)]
pub enum ExportError {
    Cancelled,
    Io(String),
}

pub fn export_rows<I>(
    dest: &Path,
    format: TransferFormat,
    options: &FormatOptions,
    columns: &[String],
    rows: I,
    cancel: &AtomicBool,
    mut progress: impl FnMut(ExportProgress),
) -> Result<ExportProgress, ExportError>
where
    I: IntoIterator<Item = Vec<DbValue>>,
{
    let dir = dest
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp =
        NamedTempFile::new_in(dir).map_err(|error| ExportError::Io(describe_io(&error, dir)))?;
    // An SQL export has to name the table it inserts into: without one, the file's name
    // (`orders.sql` inserts into `orders`), so it replays as it is.
    let named;
    let options = match (&options.table, dest.file_stem()) {
        (None, Some(stem)) => {
            named = FormatOptions {
                table: Some(stem.to_string_lossy().into_owned()),
                ..options.clone()
            };
            &named
        }
        _ => options,
    };
    let mut rows_written = 0u64;
    let mut bytes = 0u64;
    let cancelled = {
        let mut encoder = StreamEncoder::new(tmp.as_file_mut(), format, options, columns)
            .map_err(ExportError::Io)?;
        let mut cancelled = false;
        for row in rows {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            encoder.write_row(&row).map_err(ExportError::Io)?;
            rows_written += 1;
            if rows_written.is_multiple_of(1024) {
                progress(ExportProgress {
                    rows: rows_written,
                    bytes: encoder.bytes_written(),
                });
            }
        }
        if !cancelled {
            bytes = encoder.finish().map_err(ExportError::Io)?;
        }
        cancelled
    };
    if cancelled {
        let _ = tmp.close();
        return Err(ExportError::Cancelled);
    }
    tmp.as_file()
        .sync_all()
        .or_else(|_| tmp.as_file_mut().flush())
        .map_err(|error| ExportError::Io(error.to_string()))?;
    tmp.persist(dest)
        .map_err(|error| ExportError::Io(describe_io(&error.error, dir)))?;
    let report = ExportProgress {
        rows: rows_written,
        bytes,
    };
    progress(report.clone());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::{ExportError, export_rows};
    use crate::transfer::codec::{FormatOptions, TransferFormat};
    use dexo_driver_api::DbValue;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn cancel_after_10k_leaves_destination_and_removes_temp() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.csv");
        std::fs::write(&dest, b"keep").unwrap();
        let cancel = AtomicBool::new(false);
        let result = export_rows(
            &dest,
            TransferFormat::Csv,
            &FormatOptions::default(),
            &["n".into()],
            (0..1_000_000).map(|i| {
                if i == 10_000 {
                    cancel.store(true, Ordering::Relaxed);
                }
                vec![DbValue::I64(i)]
            }),
            &cancel,
            |_| {},
        );
        assert!(matches!(result, Err(ExportError::Cancelled)));
        assert_eq!(std::fs::read(&dest).unwrap(), b"keep");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path() != dest)
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn million_row_stream_does_not_collect() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.csv");
        let cancel = AtomicBool::new(false);
        let report = export_rows(
            &dest,
            TransferFormat::Csv,
            &FormatOptions::default(),
            &["n".into()],
            (0..1_000_000).map(|i| vec![DbValue::I64(i)]),
            &cancel,
            |_| {},
        )
        .unwrap();
        assert_eq!(report.rows, 1_000_000);
        assert!(dest.exists());
        assert!(std::fs::metadata(&dest).unwrap().len() > 1_000_000);
    }

    /// A folder that is not there says so, in words, and the progress counts bytes.
    #[test]
    fn a_missing_folder_is_named_and_progress_counts_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let missing = dir.path().join("nodir").join("x.csv");
        let error = export_rows(
            &missing,
            TransferFormat::Csv,
            &FormatOptions::default(),
            &["n".into()],
            [vec![DbValue::I64(1)]],
            &cancel,
            |_| {},
        )
        .unwrap_err();
        assert!(
            matches!(&error, ExportError::Io(message)
                if message.contains("nodir") && message.contains("does not exist")),
            "{error:?}"
        );
        let dest = dir.path().join("x.csv");
        let report = export_rows(
            &dest,
            TransferFormat::Csv,
            &FormatOptions::default(),
            &["n".into()],
            [vec![DbValue::I64(1)], vec![DbValue::I64(2)]],
            &cancel,
            |_| {},
        )
        .unwrap();
        assert_eq!(report.bytes, std::fs::metadata(&dest).unwrap().len());
    }

    /// An SQL export inserts into the table it is given, or else the one its file is
    /// named after, so it replays without editing.
    #[test]
    fn an_sql_export_names_its_table() {
        let dir = tempfile::tempdir().unwrap();
        let export = |file: &str, table: Option<&str>| {
            let dest = dir.path().join(file);
            let options = FormatOptions {
                table: table.map(str::to_string),
                ..FormatOptions::default()
            };
            export_rows(
                &dest,
                TransferFormat::Sql,
                &options,
                &["n".into()],
                [vec![DbValue::I64(1)]],
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
            std::fs::read_to_string(dest).unwrap()
        };
        assert_eq!(
            export("orders.sql", None),
            "INSERT INTO \"orders\" (\"n\") VALUES (1);\n"
        );
        assert_eq!(
            export("out.sql", Some("shop.order items")),
            "INSERT INTO \"shop\".\"order items\" (\"n\") VALUES (1);\n"
        );
    }
}
