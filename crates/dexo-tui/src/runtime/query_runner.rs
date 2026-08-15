use std::sync::Arc;
use std::time::Duration;

use dexo_app::{QueryService, ScriptPolicy};
use dexo_driver_api::{
    DriverError, DriverErrorCategory, QueryEvent, QueryId, QueryRequest, Session,
};
use dexo_runtime::RuntimeTaskId;
use dexo_sql::{StatementEffect, split_statements};

use crate::action::{Action, ScriptRequest};
use crate::runtime::{OperationId, OperationKey};

pub struct LiveQuery {
    pub task: RuntimeTaskId,
    pub query: QueryId,
    pub session: Arc<dyn Session>,
}

pub async fn run_script(
    query: QueryService,
    session: Arc<dyn Session>,
    request: ScriptRequest,
    action_tx: tokio::sync::mpsc::Sender<Action>,
    live: Arc<tokio::sync::Mutex<Option<LiveQuery>>>,
) {
    let key = request.key.clone();
    let _ = action_tx
        .send(Action::OperationStarted(key.clone()))
        .await;
    let mut failed = false;
    for (index, sql) in request.statements.iter().enumerate() {
        if failed && request.policy == ScriptPolicy::StopOnError {
            break;
        }
        let effect = split_statements(sql)
            .first()
            .map(|span| span.effect)
            .unwrap_or(StatementEffect::Unknown);
        let mutating = !matches!(effect, StatementEffect::ReadOnly);
        // ponytail: mutating statements run once; network failure never retries them.
        let mut query_request = if mutating {
            QueryRequest::write(sql.clone())
        } else {
            QueryRequest::read(sql.clone(), 10_000)
        };
        query_request.parameters = request.parameters.clone();
        query_request.timeout = request.timeout;
        let timeout = if request.timeout == Duration::ZERO {
            Duration::from_secs(30)
        } else {
            request.timeout
        };
        let mut task = query.start(Arc::clone(&session), query_request).await;
        {
            let mut slot = live.lock().await;
            *slot = Some(LiveQuery {
                task: task.task,
                query: task.query,
                session: Arc::clone(&session),
            });
        }
        let _ = action_tx
            .send(Action::QueryResultSetStarted {
                key: key.clone(),
                index,
            })
            .await;
        let consume = async {
            while let Some(item) = task.events.recv().await {
                match item {
                    Ok(event) => {
                        if forward_event(&action_tx, &key, index, event).await {
                            return true;
                        }
                    }
                    Err(error) => {
                        let _ = action_tx
                            .send(Action::OperationFailed {
                                key: key.clone(),
                                message: error.to_string(),
                            })
                            .await;
                        return true;
                    }
                }
            }
            false
        };
        match tokio::time::timeout(timeout, consume).await {
            Ok(true) => failed = true,
            Ok(false) => {}
            Err(_) => {
                let _ = session.cancel(task.query).await;
                query.registry().cancel(task.task);
                let _ = action_tx
                    .send(Action::OperationFailed {
                        key: key.clone(),
                        message: DriverError::new(DriverErrorCategory::Timeout, "query timed out")
                            .to_string(),
                    })
                    .await;
                failed = true;
            }
        }
    }
    if !failed {
        let _ = action_tx.send(Action::ScriptFinished { key }).await;
    }
}

async fn forward_event(
    action_tx: &tokio::sync::mpsc::Sender<Action>,
    key: &OperationKey,
    index: usize,
    event: QueryEvent,
) -> bool {
    let action = match event {
        QueryEvent::ResultSetStarted { .. } => None,
        QueryEvent::Columns(columns) => Some(Action::QueryMeta {
            key: key.clone(),
            columns,
        }),
        QueryEvent::Rows(batch) => Some(Action::QueryRows {
            key: key.clone(),
            rows: batch.rows,
        }),
        QueryEvent::Notice { message } => Some(Action::QueryNotice {
            key: key.clone(),
            message,
        }),
        QueryEvent::ResultSetFinished { rows_affected, .. } => {
            Some(Action::QueryResultSetFinished {
                key: key.clone(),
                index,
                rows_affected,
            })
        }
        QueryEvent::Finished { .. } => None,
    };
    if let Some(action) = action {
        action_tx.send(action).await.is_err()
    } else {
        false
    }
}

pub async fn cancel_live(
    query: &QueryService,
    live: &tokio::sync::Mutex<Option<LiveQuery>>,
    _id: OperationId,
) {
    if let Some(active) = live.lock().await.take() {
        query.registry().cancel(active.task);
        let _ = active.session.cancel(active.query).await;
    }
}
