use std::sync::Arc;

use dexo_driver_api::{DriverError, DriverErrorCategory, QueryEvent, QueryRequest, Session};
use dexo_runtime::{TaskRegistry, bounded_events};
use futures_util::StreamExt;
use tokio::sync::mpsc::Receiver;

use crate::error::{AppError, ErrorCategory};

pub struct QueryService {
    registry: Arc<TaskRegistry>,
}

impl QueryService {
    pub fn new(registry: Arc<TaskRegistry>) -> Self {
        Self { registry }
    }

    pub async fn start(
        &self,
        session: Arc<dyn Session>,
        request: QueryRequest,
    ) -> Receiver<Result<QueryEvent, DriverError>> {
        let handle = self.registry.register();
        let (tx, rx) = bounded_events(2);
        let token = handle.token.clone();
        let task_id = handle.id;
        let query_id = request.id;
        let registry = Arc::clone(&self.registry);
        tokio::spawn(async move {
            let run = async {
                let mut stream = match session.execute(request).await {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = tx.send(Err(error)).await;
                        return;
                    }
                };
                while let Some(event) = stream.next().await {
                    if tx.send(event).await.is_err() {
                        return;
                    }
                }
            };
            tokio::select! {
                _ = token.cancelled() => {
                    let _ = session.cancel(query_id).await;
                    let _ = tx
                        .send(Err(DriverError::new(
                            DriverErrorCategory::Cancelled,
                            "query cancelled",
                        )))
                        .await;
                }
                _ = run => {}
            }
            registry.finish(task_id);
        });
        rx
    }

    pub async fn collect(
        &self,
        session: Arc<dyn Session>,
        request: QueryRequest,
    ) -> Result<Vec<QueryEvent>, AppError> {
        let mut rx = self.start(session, request).await;
        let mut events = Vec::new();
        while let Some(item) = rx.recv().await {
            events.push(item.map_err(map_driver_error)?);
        }
        Ok(events)
    }
}

pub fn map_driver_error(error: DriverError) -> AppError {
    let category = match error.category() {
        DriverErrorCategory::Cancelled => ErrorCategory::Cancelled,
        DriverErrorCategory::Timeout => ErrorCategory::Timeout,
        DriverErrorCategory::Permission => ErrorCategory::Permission,
        DriverErrorCategory::Network | DriverErrorCategory::Transport => ErrorCategory::Network,
        DriverErrorCategory::Syntax => ErrorCategory::Syntax,
        _ => ErrorCategory::Internal,
    };
    AppError::new(category, error.to_string())
}
