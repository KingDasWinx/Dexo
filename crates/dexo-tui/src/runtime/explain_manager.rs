use dexo_driver_api::{ExplainRequest, Session};
use dexo_sql::{Dialect, statement_at_in};

/// The statement under the cursor -- the one Run would take -- without its `;`.
pub fn statement_sql(document: &str, cursor: usize, dialect: Dialect) -> Option<String> {
    let span = statement_at_in(document, cursor, dialect)?;
    let sql = document[span.byte_range].trim().trim_end_matches(';');
    (!sql.is_empty()).then(|| sql.to_string())
}

/// What one explain was asked for, and where its plan goes.
pub struct ExplainRun {
    pub cursor: usize,
    pub dialect: Dialect,
    pub analyze: bool,
    pub indexes: Vec<String>,
    pub document: String,
    pub operation: crate::runtime::OperationId,
}

pub async fn run_live(
    session: std::sync::Arc<dyn Session>,
    text: &str,
    run: ExplainRun,
    tx: tokio::sync::mpsc::Sender<crate::action::Action>,
) {
    let ExplainRun {
        cursor,
        dialect,
        analyze,
        indexes,
        document,
        operation,
    } = run;
    let statement = statement_sql(text, cursor, dialect);
    let outcome = match (&statement, session.explain()) {
        (None, _) => Err("there is no statement under the cursor to explain".to_string()),
        (Some(_), None) => Err("explain is unavailable for this connection".into()),
        (Some(sql), Some(provider)) => {
            let request = if !indexes.is_empty() {
                ExplainRequest::with_indexes(sql.clone(), indexes.clone())
            } else if analyze {
                ExplainRequest::analyzed(sql.clone())
            } else {
                ExplainRequest::estimated(sql.clone())
            };
            provider
                .explain(request)
                .await
                .map_err(|error| error.to_string())
        }
    };
    let action = match outcome {
        Ok(plan) => crate::action::Action::ExplainLoaded {
            plan: Box::new(plan),
            sql: statement.unwrap_or_default(),
            indexes,
            document,
            operation,
        },
        Err(message) => crate::action::Action::ExplainFailed {
            document,
            operation,
            message,
        },
    };
    let _ = tx.send(action).await;
}
