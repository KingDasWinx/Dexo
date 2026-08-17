use dexo_app::diagnostic_service::DiagnosticBundle;
use std::path::PathBuf;

use crate::action::Action;

#[derive(Default)]
pub struct DiagnosticManager {
    pub preview: Option<DiagnosticBundle>,
}

pub async fn write(bundle: DiagnosticBundle, path: PathBuf, tx: tokio::sync::mpsc::Sender<Action>) {
    let result = tokio::task::spawn_blocking(move || bundle.write_zip(&path).map(|()| path)).await;
    let action = match result {
        Ok(Ok(path)) => Action::DiagnosticsWritten { path },
        Ok(Err(error)) => Action::DiagnosticsFailed {
            message: error.to_string(),
        },
        Err(error) => Action::DiagnosticsFailed {
            message: error.to_string(),
        },
    };
    let _ = tx.send(action).await;
}
