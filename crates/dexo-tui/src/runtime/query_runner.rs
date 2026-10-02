use std::sync::Arc;
use std::time::Duration;

use dexo_app::{QueryService, ScriptPolicy};
use dexo_driver_api::{
    DriverError, DriverErrorCategory, QueryEvent, QueryId, QueryRequest, Session,
};
use dexo_runtime::RuntimeTaskId;
use dexo_sql::{StatementEffect, split_statements_in};

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
    let _ = action_tx.send(Action::OperationStarted(key.clone())).await;
    let statements = request.statements.len();
    let mut failed = false;
    for (index, sql) in request.statements.iter().enumerate() {
        if failed && request.policy == ScriptPolicy::StopOnError {
            break;
        }
        // Answered here, from the catalog: a backslash command never reaches the driver.
        if dexo_app::meta_command::is_meta(sql) {
            if let Err(error) = answer_meta(&action_tx, &key, index, session.as_ref(), sql).await {
                report_failure(&action_tx, &key, index, sql, &error, statements).await;
                failed = true;
            }
            continue;
        }
        let effect = split_statements_in(sql, request.dialect)
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
        // Whatever it is taken for, what it returns stops at the grid's limit: a
        // `with recursive` the splitter cannot read went out as a write with none.
        query_request.row_limit = 10_000;
        query_request.parameters = request.parameters.clone();
        query_request.timeout = request.timeout;
        query_request.read_only = request.read_only;
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
                        report_failure(&action_tx, &key, index, sql, &error, statements).await;
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
                let error = DriverError::new(DriverErrorCategory::Timeout, "query timed out");
                report_failure(&action_tx, &key, index, sql, &error, statements).await;
                failed = true;
            }
        }
    }
    if !failed {
        let _ = action_tx.send(Action::ScriptFinished { key }).await;
    }
}

/// Sends the failure to the model with what the server said about it, and leaves a line in
/// the log file, which until now received nothing about a failed statement. The file gets
/// the category, SQLSTATE and the server's one-line message; the statement text and the
/// DETAIL line, which quotes row values, stay out of it.
async fn report_failure(
    action_tx: &tokio::sync::mpsc::Sender<Action>,
    key: &OperationKey,
    index: usize,
    sql: &str,
    error: &DriverError,
    statements: usize,
) {
    tracing::warn!(
        category = ?error.category(),
        code = error.native_code().unwrap_or("-"),
        statement = index + 1,
        "statement failed: {error}"
    );
    let _ = action_tx
        .send(Action::QueryFailed {
            key: key.clone(),
            index,
            message: error.to_string(),
            details: crate::model::describe_query_error(sql, error, (index, statements)),
            position: error.position(),
        })
        .await;
}

/// A backslash command's answer, sent as the events a query's result set would be.
async fn answer_meta(
    action_tx: &tokio::sync::mpsc::Sender<Action>,
    key: &OperationKey,
    index: usize,
    session: &dyn Session,
    sql: &str,
) -> Result<(), DriverError> {
    use dexo_app::meta_command::{self, MetaCommand};
    let fail = |message: String| DriverError::new(DriverErrorCategory::Syntax, message);
    let command = meta_command::parse(sql).map_err(|error| fail(error.to_string()))?;
    if let MetaCommand::RecordView(on) = command {
        let action = match on {
            Some(on) => Action::SetRecordView(on),
            None => Action::ToggleRecordView,
        };
        let _ = action_tx.send(action).await;
        return Ok(());
    }
    let catalog = session.catalog().ok_or_else(|| {
        DriverError::new(
            DriverErrorCategory::Capability,
            "this connection has no catalog to answer from",
        )
    })?;
    let answer = meta_command::answer(catalog, &command)
        .await
        .map_err(|error| fail(error.to_string()))?;
    let _ = action_tx
        .send(Action::QueryResultSetStarted {
            key: key.clone(),
            index,
        })
        .await;
    let columns = answer
        .columns
        .iter()
        .map(|name| dexo_driver_api::ColumnMeta {
            name: name.to_string(),
            type_name: "text".into(),
            nullable: false,
        })
        .collect();
    let rows = answer
        .rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(dexo_driver_api::DbValue::Text)
                .collect()
        })
        .collect();
    for event in [
        QueryEvent::Columns(columns),
        QueryEvent::Rows(dexo_driver_api::RowBatch { rows }),
        QueryEvent::ResultSetFinished {
            index,
            rows_affected: None,
            truncated: false,
        },
    ] {
        if forward_event(action_tx, key, index, event).await {
            break;
        }
    }
    Ok(())
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
            index,
            columns,
        }),
        QueryEvent::Rows(batch) => Some(Action::QueryRows {
            key: key.clone(),
            index,
            rows: batch.rows,
        }),
        QueryEvent::Notice { message } => Some(Action::QueryNotice {
            key: key.clone(),
            index,
            message,
        }),
        QueryEvent::ResultSetFinished {
            rows_affected,
            truncated,
            ..
        } => Some(Action::QueryResultSetFinished {
            key: key.clone(),
            index,
            rows_affected,
            truncated,
        }),
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use dexo_driver_api::{
        CapabilityState, DriverError, QueryEvent, QueryId, QueryRequest, QueryStream, Session,
    };

    /// Remembers the row limit each statement was sent with.
    #[derive(Default)]
    struct Limits(Mutex<Vec<u64>>);

    #[async_trait::async_trait]
    impl Session for Limits {
        fn capabilities(&self) -> &[CapabilityState] {
            &[]
        }

        async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError> {
            self.0.lock().unwrap().push(request.row_limit);
            let done: Vec<Result<QueryEvent, DriverError>> = vec![Ok(QueryEvent::Finished {
                rows_affected: None,
            })];
            Ok(Box::pin(futures_util::stream::iter(done)))
        }

        async fn cancel(&self, _query: QueryId) -> Result<(), DriverError> {
            Ok(())
        }

        async fn close(self: Box<Self>) -> Result<(), DriverError> {
            Ok(())
        }
    }

    /// A statement the splitter cannot read still has its rows stopped at the limit.
    #[tokio::test]
    async fn every_statement_keeps_the_row_limit() {
        let session = Arc::new(Limits::default());
        let (action_tx, mut actions) = tokio::sync::mpsc::channel(64);
        let request = crate::action::ScriptRequest {
            key: crate::runtime::OperationKey::new(crate::runtime::OperationId::new(), "s", "d", 1),
            statements: vec![
                "with recursive t(n) as (select 1 union all select n + 1 from t) select n from t"
                    .into(),
                "select 1".into(),
            ],
            dialect: dexo_sql::Dialect::Postgres,
            policy: dexo_app::ScriptPolicy::ContinueOnError,
            parameters: Vec::new(),
            timeout: std::time::Duration::from_secs(5),
            read_only: false,
        };
        super::run_script(
            dexo_app::QueryService::new(Arc::new(dexo_runtime::TaskRegistry::default())),
            Arc::clone(&session) as Arc<dyn Session>,
            request,
            action_tx,
            Arc::new(tokio::sync::Mutex::new(None)),
        )
        .await;
        while actions.try_recv().is_ok() {}
        assert_eq!(*session.0.lock().unwrap(), [10_000, 10_000]);
    }
}
