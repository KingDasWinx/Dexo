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
    let mut tmp = NamedTempFile::new_in(dir).map_err(|error| ExportError::Io(error.to_string()))?;
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
            bytes += encoder.write_row(&row).map_err(ExportError::Io)?;
            rows_written += 1;
            if rows_written.is_multiple_of(1024) {
                progress(ExportProgress {
                    rows: rows_written,
                    bytes,
                });
            }
        }
        if !cancelled {
            encoder.finish().map_err(ExportError::Io)?;
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
        .map_err(|error| ExportError::Io(error.error.to_string()))?;
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
}
