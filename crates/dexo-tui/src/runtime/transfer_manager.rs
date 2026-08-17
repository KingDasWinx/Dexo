use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dexo_app::transfer::{
    ExportError, ExportProgress, FormatOptions, NativeHandle, NativeStatus, NativeToolKind,
    NativeToolRequest, NativeToolRunner, RecordingSink, TokioProcessRunner, decode_document,
    export_row_batches, export_rows, import_rows,
};
use dexo_driver_api::{DbValue, QualifiedName, Session};
use secrecy::SecretString;
use tokio::sync::mpsc::Sender;

use crate::action::{Action, TransferRequest};
use crate::runtime::OperationId;
use crate::screens::transfer::TransferMode;

pub enum RunningTransfer {
    Cooperative(Arc<AtomicBool>),
    Native(Box<NativeHandle>),
}

#[derive(Default)]
pub struct TransferManager {
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
    pub fn recorded_modes(&self) -> Vec<TransferMode> {
        self.recorded.clone()
    }

    pub async fn run(&mut self, request: TransferRequest) -> Result<(), String> {
        self.run_with(request, None).await
    }

    pub async fn run_with(
        &mut self,
        request: TransferRequest,
        runtime: Option<&RuntimeAccess>,
    ) -> Result<(), String> {
        self.recorded.push(request.mode());
        match request {
            TransferRequest::Export {
                operation,
                path,
                format,
                columns,
                rows,
            } => run_export(self, operation, path, format, columns, rows, runtime).await,
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

    pub async fn cancel(&mut self, operation: OperationId) -> bool {
        match self.running.remove(&operation) {
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

async fn run_export(
    manager: &mut TransferManager,
    operation: OperationId,
    path: PathBuf,
    format: dexo_app::transfer::TransferFormat,
    columns: Vec<String>,
    rows: Arc<Vec<Vec<DbValue>>>,
    runtime: Option<&RuntimeAccess>,
) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    manager
        .running
        .insert(operation, RunningTransfer::Cooperative(Arc::clone(&cancel)));
    let tx = runtime.map(|access| access.action_tx.clone());
    let cancel_for_worker = Arc::clone(&cancel);
    let result = tokio::task::spawn_blocking(move || {
        export_rows(
            &path,
            format,
            &FormatOptions::default(),
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
    manager.running.remove(&operation);
    finish_export(operation, result, runtime).await
}

async fn finish_export(
    operation: OperationId,
    result: Result<ExportProgress, ExportError>,
    runtime: Option<&RuntimeAccess>,
) -> Result<(), String> {
    match result {
        Ok(progress) => {
            emit(
                runtime,
                Action::TransferFinished {
                    operation,
                    message: format!("exported {} rows", progress.rows),
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

async fn run_import(
    manager: &mut TransferManager,
    operation: OperationId,
    path: PathBuf,
    format: dexo_app::transfer::TransferFormat,
    target: QualifiedName,
    strategy: dexo_app::transfer::ErrorStrategy,
    runtime: Option<&RuntimeAccess>,
) -> Result<(), String> {
    let Some(session) = runtime.and_then(|access| access.session.clone()) else {
        // ponytail: recording double never writes; production always supplies a session.
        return Ok(());
    };
    let Some(writer) = session.bulk() else {
        let message = "this driver does not offer bulk import".to_string();
        emit(
            runtime,
            Action::TransferFailed {
                operation,
                message: message.clone(),
            },
        )
        .await;
        return Err(message);
    };
    let cancel = Arc::new(AtomicBool::new(false));
    manager
        .running
        .insert(operation, RunningTransfer::Cooperative(Arc::clone(&cancel)));
    let decoded = tokio::task::spawn_blocking({
        let path = path.clone();
        move || {
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            decode_document(format, &FormatOptions::default(), &bytes)
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    let (columns, rows) = match decoded {
        Ok(value) => value,
        Err(message) => {
            manager.running.remove(&operation);
            emit(
                runtime,
                Action::TransferFailed {
                    operation,
                    message: message.clone(),
                },
            )
            .await;
            return Err(message);
        }
    };
    let import_rows_data: Vec<(usize, Vec<DbValue>, Vec<String>)> = rows
        .into_iter()
        .enumerate()
        .map(|(index, values)| (index + 1, values, Vec::new()))
        .collect();
    let tx = runtime.map(|access| access.action_tx.clone());
    let result = import_rows(
        writer,
        &target,
        &columns,
        import_rows_data,
        strategy,
        cancel.as_ref(),
        None,
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
    .await;
    manager.running.remove(&operation);
    match result {
        Ok(report) => {
            emit(
                runtime,
                Action::TransferFinished {
                    operation,
                    message: format!("imported {} rows", report.committed),
                },
            )
            .await;
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
        Err(message) => {
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

async fn run_native(
    manager: &mut TransferManager,
    operation: OperationId,
    path: PathBuf,
    mode: TransferMode,
    runtime: Option<&RuntimeAccess>,
) -> Result<(), String> {
    let Some(access) = runtime else {
        // ponytail: recording double records Restore/Backup without touching path.
        return Ok(());
    };
    let driver = access.driver.as_deref().unwrap_or_default();
    let kind = match (mode, driver) {
        (TransferMode::Backup, "postgres") => NativeToolKind::PgDump,
        (TransferMode::Restore, "postgres") => NativeToolKind::PgRestore,
        (TransferMode::Backup, "mysql") => NativeToolKind::MysqlDump,
        (TransferMode::Restore, "mysql") => NativeToolKind::MysqlRestore,
        _ => {
            let message = "this driver does not offer backup".to_string();
            emit(
                runtime,
                Action::TransferFailed {
                    operation,
                    message: message.clone(),
                },
            )
            .await;
            return Err(message);
        }
    };
    let request = NativeToolRequest {
        kind,
        host: access.host.clone().unwrap_or_else(|| "localhost".into()),
        port: access.port.unwrap_or(5432),
        database: access.database.clone().unwrap_or_default(),
        username: access.username.clone().unwrap_or_default(),
        path,
        secret: access
            .secret
            .clone()
            .unwrap_or_else(|| SecretString::from(String::new())),
        expected_major: 0,
    };
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let runner = NativeToolRunner::<TokioProcessRunner>::new(TokioProcessRunner);
    let handle = runner
        .start(request, "0", dir.path())
        .await
        .map_err(|error: dexo_app::transfer::NativeToolError| error.to_string())?;
    manager
        .running
        .insert(operation, RunningTransfer::Native(Box::new(handle)));
    let Some(RunningTransfer::Native(handle)) = manager.running.remove(&operation) else {
        return Ok(());
    };
    let result = handle.outcome().await.map_err(|error| error.to_string())?;
    match result.status {
        NativeStatus::Succeeded => {
            emit(
                runtime,
                Action::TransferFinished {
                    operation,
                    message: format!("{} completed", mode.as_str()),
                },
            )
            .await;
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
            let message = result.sanitized_log;
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
        let (host, port) = dexo_driver_api::split_endpoint(&connect.endpoint)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            action_tx,
            session: Some(session),
            driver: Some(profile.driver.clone()),
            host: Some(host),
            port: Some(port),
            database: connect.database.clone(),
            username: Some(connect.username.clone()),
            secret: Some(secret),
        })
    }
}
