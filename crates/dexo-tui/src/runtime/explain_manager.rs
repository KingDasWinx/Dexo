use dexo_driver_api::{ExplainRequest, Session};
use dexo_sql::statement_at;

/// The statement under the cursor -- the one Run would take -- without its `;`.
pub fn statement_sql(document: &str, cursor: usize) -> Option<String> {
    let span = statement_at(document, cursor)?;
    let sql = document[span.byte_range].trim().trim_end_matches(';');
    (!sql.is_empty()).then(|| sql.to_string())
}

pub async fn run_live(
    session: std::sync::Arc<dyn Session>,
    document: &str,
    cursor: usize,
    analyze: bool,
    operation: crate::runtime::OperationId,
    tx: tokio::sync::mpsc::Sender<crate::action::Action>,
) {
    let outcome = match (statement_sql(document, cursor), session.explain()) {
        (None, _) => Err("there is no statement under the cursor to explain".to_string()),
        (Some(_), None) => Err("explain is unavailable for this connection".into()),
        (Some(sql), Some(provider)) => {
            let request = if analyze {
                ExplainRequest::analyzed(sql)
            } else {
                ExplainRequest::estimated(sql)
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
            operation,
        },
        Err(message) => crate::action::Action::ExplainFailed { operation, message },
    };
    let _ = tx.send(action).await;
}
