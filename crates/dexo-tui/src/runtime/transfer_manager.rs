use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use dexo_app::transfer::{
    ErrorStrategy, ExportError, ExportProgress, FormatOptions, NativeHandle, NativeStatus,
    NativeToolKind, NativeToolRequest, NativeToolRunner, RecordingSink, TokioProcessRunner,
    decode_document, export_row_batches, export_rows, fit_to_table, import_rows, target_columns,
};
use dexo_driver_api::{DbValue, QualifiedName, Session};
use secrecy::SecretString;
use tokio::sync::mpsc::Sender;

use crate::action::{Action, TransferRequest};
use crate::runtime::OperationId;
use crate::screens::transfer::{TransferMode, rows_of, size_of};

#[derive(Clone)]
pub enum RunningTransfer {
    Cooperative(Arc<AtomicBool>),
    Native(Arc<NativeHandle>),
}

/// The transfers under way, by operation. Cloned into the tasks that run them: a transfer
/// takes as long as the data does, so none of it runs on the loop that draws the screen.
#[derive(Clone, Default)]
pub struct TransferManager {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
struct Shared {
    running: HashMap<OperationId, RunningTransfer>,
    recorded: Vec<TransferMode>,
}

pub struct RuntimeAccess {
    pub action_tx: Sender<Action>,
    pub session: Option<Arc<dyn Session>>,
    pub driver: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub username: Option<String>,
    pub secret: Option<SecretString>,
}

impl TransferManager {
    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn start(&self, operation: OperationId, running: RunningTransfer) {
        self.shared().running.insert(operation, running);
    }

    fn finish(&self, operation: OperationId) {
        self.shared().running.remove(&operation);
    }

    pub fn recorded_modes(&self) -> Vec<TransferMode> {
        self.shared().recorded.clone()
    }

    pub async fn run(&self, request: TransferRequest) -> Result<(), String> {
        self.run_with(request, None).await
    }

    pub async fn run_with(
        &self,
        request: TransferRequest,
        runtime: Option<&RuntimeAccess>,
    ) -> Result<(), String> {
        self.shared().recorded.push(request.mode());
        match request {
            TransferRequest::Export {
                operation,
                path,
                format,
                columns,
                rows,
                dialect,
                table,
            } => {
                let options = FormatOptions {
                    dialect,
                    table,
                    ..FormatOptions::default()
                };
                run_export(
                    self, operation, path, format, &options, columns, rows, runtime,
                )
                .await
            }
            TransferRequest::Import {
                operation,
                path,
                format,
                target,
                strategy,
                session: _,
            } => run_import(self, operation, path, format, target, strategy, runtime).await,
            TransferRequest::Backup {
                operation,
                path,
                session: _,
            } => run_native(self, operation, path, TransferMode::Backup, runtime).await,
            TransferRequest::Restore {
                operation,
                path,
                session: _,
            } => run_native(self, operation, path, TransferMode::Restore, runtime).await,
        }
    }

    /// Stops a transfer under way. A native tool is killed; an export or an import stops
    /// at its next row or batch.
    pub async fn cancel(&self, operation: OperationId) -> bool {
        let running = self.shared().running.get(&operation).cloned();
        match running {
            Some(RunningTransfer::Cooperative(token)) => {
                token.store(true, Ordering::Release);
                true
            }
            Some(RunningTransfer::Native(handle)) => handle.cancel().await.is_ok(),
            None => false,
        }
    }

    pub async fn export_batches(
        batches: impl IntoIterator<Item = Vec<Vec<DbValue>>>,
        sink: RecordingSink,
    ) -> Result<(), ExportError> {
        export_row_batches(batches, sink).await
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_export(
    manager: &TransferManager,
    operation: OperationId,
    path: PathBuf,
    format: dexo_app::transfer::TransferFormat,
    options: &FormatOptions,
    columns: Vec<String>,
    rows: Arc<Vec<Vec<DbValue>>>,
    runtime: Option<&RuntimeAccess>,
) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let options = options.clone();
    manager.start(operation, RunningTransfer::Cooperative(Arc::clone(&cancel)));
    let tx = runtime.map(|access| access.action_tx.clone());
    let cancel_for_worker = Arc::clone(&cancel);
    let written = path.clone();
    let result = tokio::task::spawn_blocking(move || {
        export_rows(
            &path,
            format,
            &options,
            &columns,
            rows.iter().cloned(),
            cancel_for_worker.as_ref(),
            |progress: ExportProgress| {
                if let Some(tx) = &tx {
                    let _ = tx.blocking_send(Action::TransferProgress {
                        operation,
                        rows: progress.rows,
                        bytes: progress.bytes,
                    });
                }
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?;
    manager.finish(operation);
    finish_export(operation, result, &written, runtime).await
}

async fn finish_export(
    operation: OperationId,
    result: Result<ExportProgress, ExportError>,
    written: &Path,
    runtime: Option<&RuntimeAccess>,
) -> Result<(), String> {
    match result {
        Ok(progress) => {
            emit(
                runtime,
                Action::TransferFinished {
                    operation,
                    message: format!(
                        "Exported {} to {} ({}).",
                        rows_of(progress.rows),
                        written.display(),
                        size_of(progress.bytes)
                    ),
                },
            )
            .await;
            Ok(())
        }
        Err(ExportError::Cancelled) => {
            emit(
                runtime,
                Action::OperationCancelled(operation_key(operation)),
            )
            .await;
            Err("cancelled".into())
        }
        Err(ExportError::Io(message)) => {
            emit(
                runtime,
                Action::TransferFailed {
                    operation,
                    message: message.clone(),
                },
            )
            .await;
            Err(message)
        }
    }
}

async fn fail(operation: OperationId, runtime: Option<&RuntimeAccess>, message: String) -> String {
    emit(
        runtime,
        Action::TransferFailed {
            operation,
            message: message.clone(),
        },
    )
    .await;
    message
}

async fn run_import(
    manager: &TransferManager,
    operation: OperationId,
    path: PathBuf,
    format: dexo_app::transfer::TransferFormat,
    target: QualifiedName,
    strategy: ErrorStrategy,
    runtime: Option<&RuntimeAccess>,
) -> Result<(), String> {
    let Some(session) = runtime.and_then(|access| access.session.clone()) else {
        // ponytail: recording double never writes; production always supplies a session.
        return Ok(());
    };
    let Some(writer) = session.bulk() else {
        let message = "This connection cannot import data.".to_string();
        return Err(fail(operation, runtime, message).await);
    };
    let cancel = Arc::new(AtomicBool::new(false));
    manager.start(operation, RunningTransfer::Cooperative(Arc::clone(&cancel)));
    let result = import_file(
        &session, writer, &path, format, &target, strategy, &cancel, operation, runtime,
    )
    .await;
    manager.finish(operation);
    match result {
        Ok(message) => {
            emit(runtime, Action::TransferFinished { operation, message }).await;
            Ok(())
        }
        Err(message) if message == "cancelled" => {
            emit(
                runtime,
                Action::OperationCancelled(operation_key(operation)),
            )
            .await;
            Err(message)
        }
        Err(message) => Err(fail(operation, runtime, message).await),
    }
}

/// Reads `path`, fits its rows to the table and writes them. The error is a sentence for
/// the dialog.
#[allow(clippy::too_many_arguments)]
async fn import_file(
    session: &Arc<dyn Session>,
    writer: &dyn dexo_driver_api::BulkWriter,
    path: &Path,
    format: dexo_app::transfer::TransferFormat,
    target: &QualifiedName,
    strategy: ErrorStrategy,
    cancel: &AtomicBool,
    operation: OperationId,
    runtime: Option<&RuntimeAccess>,
) -> Result<String, String> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let decoded = tokio::task::spawn_blocking({
        let path = path.to_path_buf();
        let name = name.clone();
        move || {
            let bytes = std::fs::read(&path)
                .map_err(|error| format!("{} cannot be read: {error}.", path.display()))?;
            decode_document(format, &FormatOptions::default(), &bytes).map_err(|error| {
                format!(
                    "{name} could not be read as {}: {error}. Choose the format that matches the file.",
                    format_name(format)
                )
            })
        }
    })
    .await
    .map_err(|error| error.to_string())??;
    let (mut columns, mut rows) = decoded;
    let table = target.display_unquoted();
    // The table's own columns: the file's are checked against them and spelled as the
    // table spells them, and a blank cell in a number column is NULL.
    if let Some(catalog) = session.catalog() {
        let known = target_columns(catalog, &table).await?;
        (columns, rows) = fit_to_table(&table, columns, rows, &known)?;
    }
    // The line of the file each row is on: the header is the first line of a CSV or TSV.
    let first_line = match format {
        dexo_app::transfer::TransferFormat::Csv | dexo_app::transfer::TransferFormat::Tsv => 2,
        _ => 1,
    };
    let rows: Vec<(usize, Vec<DbValue>, Vec<String>)> = rows
        .into_iter()
        .enumerate()
        .map(|(index, values)| {
            let original = values.iter().map(original_text).collect();
            (index + first_line, values, original)
        })
        .collect();
    let rejects = (strategy == ErrorStrategy::RejectFile)
        .then(|| path.with_file_name(format!("{name}.rejects.csv")));
    let tx = runtime.map(|access| access.action_tx.clone());
    let report = import_rows(
        writer,
        target,
        &columns,
        rows,
        strategy,
        cancel,
        rejects.as_deref(),
        |rows| {
            if let Some(tx) = &tx {
                let _ = tx.try_send(Action::TransferProgress {
                    operation,
                    rows,
                    bytes: 0,
                });
            }
        },
    )
    .await?;
    let mut message = format!("Imported {} into {table}.", rows_of(report.committed));
    if report.skipped > 0 {
        message.push_str(&format!(" {} skipped.", rows_of(report.skipped)));
    }
    if let Some(rejects) = rejects.filter(|_| !report.rejected.is_empty()) {
        message.push_str(&format!(
            " {} set aside in {}.",
            rows_of(report.rejected.len() as u64),
            rejects.display()
        ));
    }
    Ok(message)
}

fn format_name(format: dexo_app::transfer::TransferFormat) -> &'static str {
    use dexo_app::transfer::TransferFormat;
    match format {
        TransferFormat::Csv => "CSV",
        TransferFormat::Tsv => "TSV",
        TransferFormat::Json => "JSON",
        TransferFormat::Jsonl => "JSON Lines",
        TransferFormat::Sql => "SQL",
    }
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
        DbValue::Bytes(bytes) => {
            format!(
                "\\x{}",
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            )
        }
    }
}

async fn run_native(
    manager: &TransferManager,
    operation: OperationId,
    path: PathBuf,
    mode: TransferMode,
    runtime: Option<&RuntimeAccess>,
) -> Result<(), String> {
    let Some(access) = runtime else {
        // ponytail: recording double records Restore/Backup without touching path.
        return Ok(());
    };
    let driver =
        dexo_driver_api::DriverDescriptor::family(access.driver.as_deref().unwrap_or_default());
    let kind = match (mode, driver) {
        (TransferMode::Backup, "postgres") => NativeToolKind::PgDump,
        (TransferMode::Restore, "postgres") => NativeToolKind::PgRestore,
        (TransferMode::Backup, "mysql") => NativeToolKind::MysqlDump,
        (TransferMode::Restore, "mysql") => NativeToolKind::MysqlRestore,
        _ => {
            let message = format!("Dexo has no {} for this kind of database.", mode.as_str());
            return Err(fail(operation, runtime, message).await);
        }
    };
    let request = NativeToolRequest {
        kind,
        host: access.host.clone().unwrap_or_else(|| "localhost".into()),
        port: access.port.unwrap_or(5432),
        database: access.database.clone().unwrap_or_default(),
        username: access.username.clone().unwrap_or_default(),
        path: path.clone(),
        secret: access
            .secret
            .clone()
            .unwrap_or_else(|| SecretString::from(String::new())),
        expected_major: 0,
    };
    // A cancel that comes before the tool is running is kept, and acted on once it is.
    let cancel = Arc::new(AtomicBool::new(false));
    manager.start(operation, RunningTransfer::Cooperative(Arc::clone(&cancel)));
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let runner = NativeToolRunner::<TokioProcessRunner>::new(TokioProcessRunner);
    let handle = match runner.start(request, "0", dir.path()).await {
        Ok(handle) => Arc::new(handle),
        Err(error) => {
            manager.finish(operation);
            return Err(fail(operation, runtime, error.to_string()).await);
        }
    };
    manager.start(operation, RunningTransfer::Native(Arc::clone(&handle)));
    if cancel.load(Ordering::Acquire) {
        let _ = handle.cancel().await;
    }
    // The size of a backup as it grows is the progress there is to show; a restore has
    // only the time it has taken, which the dialog counts.
    let ticker = tokio::spawn({
        let handle = Arc::clone(&handle);
        let tx = access.action_tx.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let _ = tx
                    .send(Action::TransferProgress {
                        operation,
                        rows: 0,
                        bytes: handle.written_bytes(),
                    })
                    .await;
            }
        }
    });
    let outcome = handle.outcome().await;
    ticker.abort();
    manager.finish(operation);
    let result = match outcome {
        Ok(result) => result,
        Err(error) => return Err(fail(operation, runtime, error.to_string()).await),
    };
    match result.status {
        NativeStatus::Succeeded => {
            let message = match (mode, result.ignored_errors) {
                (TransferMode::Backup, _) => {
                    let size = std::fs::metadata(&path).map_or(0, |metadata| metadata.len());
                    format!("Backup written to {} ({}).", path.display(), size_of(size))
                }
                (_, Some(ignored)) => format!(
                    "Restored from {} with {ignored} ignored error{}. First: {}",
                    path.display(),
                    if ignored == 1 { "" } else { "s" },
                    first_error(&result.sanitized_log)
                ),
                (_, None) => format!("Restored from {}.", path.display()),
            };
            emit(runtime, Action::TransferFinished { operation, message }).await;
            Ok(())
        }
        NativeStatus::Cancelled => {
            emit(
                runtime,
                Action::OperationCancelled(operation_key(operation)),
            )
            .await;
            Err("cancelled".into())
        }
        NativeStatus::Failed | NativeStatus::Running => {
            let tool = result
                .command_line
                .split_whitespace()
                .next()
                .unwrap_or("The tool");
            let message = failure_text(tool, &result.sanitized_log);
            Err(fail(operation, runtime, message).await)
        }
    }
}

/// What the tool said first that was an error, without its program prefix.
fn first_error(log: &str) -> String {
    log.lines()
        .find(|line| line.to_ascii_lowercase().contains("error"))
        .map(|line| line.split_once(": ").map_or(line, |(_, rest)| rest))
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            // `pg_restore: error: x` and `psql:file.sql:7: ERROR:  x` both say error once.
            if lower.starts_with("error: ") || lower.starts_with("error:") {
                &line[line.find(':').map_or(0, |at| at + 1)..]
            } else {
                line
            }
        })
        .unwrap_or("see the tool's output")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The tool's own last words as the reason: `pg_restore: error: ...`, not a command line.
fn failure_text(tool: &str, log: &str) -> String {
    let lines: Vec<String> = log
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect();
    let tail = lines[lines.len().saturating_sub(4)..].join(" ");
    if tail.is_empty() {
        format!("{tool} failed without saying why.")
    } else {
        format!("{tool} failed: {tail}")
    }
}

fn operation_key(operation: OperationId) -> crate::runtime::OperationKey {
    crate::runtime::OperationKey::new(operation, "", "", 0)
}

async fn emit(runtime: Option<&RuntimeAccess>, action: Action) {
    if let Some(access) = runtime {
        let _ = access.action_tx.send(action).await;
    }
}

impl RuntimeAccess {
    pub fn from_profile(
        action_tx: Sender<Action>,
        session: Arc<dyn Session>,
        profile: &dexo_app::ConnectionProfile,
        secret: SecretString,
    ) -> Result<Self, String> {
        let (connect, _) = profile
            .connect_request(secret.clone())
            .map_err(|error| error.to_string())?;
        // A file's endpoint is its path; there is no host for a native tool to dial.
        let (host, port) = if profile.is_file() {
            (None, None)
        } else {
            let (host, port) = dexo_driver_api::split_endpoint(&connect.endpoint)
                .map_err(|error| error.to_string())?;
            (Some(host), Some(port))
        };
        Ok(Self {
            action_tx,
            session: Some(session),
            driver: Some(profile.driver.clone()),
            host,
            port,
            database: connect.database.clone(),
            username: Some(connect.username.clone()),
            secret: Some(secret),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{failure_text, first_error};

    #[test]
    fn a_failure_is_the_tools_own_words() {
        let log = "pg_restore: error: connection to server at \"127.0.0.1\", port 5432 failed: FATAL:  password authentication failed for user \"dexo\"\n";
        assert_eq!(
            failure_text("pg_restore", log),
            "pg_restore failed: pg_restore: error: connection to server at \"127.0.0.1\", port 5432 failed: FATAL: password authentication failed for user \"dexo\""
        );
        assert_eq!(
            failure_text("mysqldump", ""),
            "mysqldump failed without saying why."
        );
    }

    #[test]
    fn a_scripts_first_error_is_read_like_a_restores() {
        assert_eq!(
            first_error("psql:shop.sql:7: ERROR:  relation \"t\" already exists"),
            "relation \"t\" already exists"
        );
    }

    #[test]
    fn the_first_error_loses_its_program_prefix() {
        let log = "pg_restore: error: could not execute query: ERROR:  unrecognized configuration parameter \"transaction_timeout\"\npg_restore: warning: errors ignored on restore: 1";
        assert_eq!(
            first_error(log),
            "could not execute query: ERROR: unrecognized configuration parameter \"transaction_timeout\""
        );
    }
}
