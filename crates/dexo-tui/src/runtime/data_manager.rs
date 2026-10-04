use std::sync::Arc;

use dexo_driver_api::{DataRequest, Filter, Page, QualifiedName, Session, Sort};

use crate::action::Action;
use crate::runtime::{OperationId, SessionId};

pub async fn fetch_page(
    session: Arc<dyn Session>,
    request: DataRequest,
    generation: u64,
    ticket: OperationId,
    session_id: SessionId,
    action_tx: tokio::sync::mpsc::Sender<Action>,
) {
    let Some(data) = session.data() else {
        let _ = action_tx
            .send(Action::DataPageFailed {
                generation,
                ticket,
                message: "data capability unavailable".into(),
            })
            .await;
        return;
    };
    // Asked only of a first page that leaves rows out and has nothing filtering it: what
    // the statistics say is the table's size, not the filter's. The pages after it keep
    // the one the first got.
    let unfiltered =
        request.filter.is_none() && request.clauses.where_sql.is_none() && request.page.offset == 0;
    let object = request.object.clone();
    match data.fetch(request).await {
        Ok(mut page) => {
            if page.has_more && unfiltered {
                page.estimated_total = data.estimate_rows(&object).await.ok().flatten();
            }
            let _ = action_tx
                .send(Action::DataPageLoaded {
                    generation,
                    session: session_id.0.to_string(),
                    ticket,
                    page,
                })
                .await;
        }
        Err(error) => {
            let _ = action_tx
                .send(Action::DataPageFailed {
                    generation,
                    ticket,
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub async fn fetch_table_columns(
    session: Arc<dyn Session>,
    target: QualifiedName,
    generation: u64,
    ticket: OperationId,
    action_tx: tokio::sync::mpsc::Sender<Action>,
) {
    let Some(data) = session.data() else {
        let _ = action_tx
            .send(Action::TableColumnsFailed {
                generation,
                ticket,
                message: "data capability unavailable".into(),
            })
            .await;
        return;
    };
    match data.table_columns(&target).await {
        Ok(columns) => {
            let _ = action_tx
                .send(Action::TableColumnsLoaded {
                    generation,
                    ticket,
                    columns,
                })
                .await;
        }
        Err(error) => {
            let _ = action_tx
                .send(Action::TableColumnsFailed {
                    generation,
                    ticket,
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub async fn apply_mutations(
    session: Arc<dyn Session>,
    mutations: Vec<dexo_driver_api::Mutation>,
    generation: u64,
    session_id: SessionId,
    action_tx: tokio::sync::mpsc::Sender<Action>,
) {
    let Some(data) = session.data() else {
        let _ = action_tx
            .send(Action::MutationsFailed {
                generation,
                message: "data capability unavailable".into(),
            })
            .await;
        return;
    };
    match data.apply(&mutations).await {
        Ok(()) => {
            let _ = action_tx
                .send(Action::MutationsApplied {
                    generation,
                    session: session_id.0.to_string(),
                })
                .await;
        }
        Err(error) => {
            let _ = action_tx
                .send(Action::MutationsFailed {
                    generation,
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub async fn fetch_value(
    session: Arc<dyn Session>,
    value: dexo_driver_api::RemoteValueRef,
    offset: u64,
    limit: u32,
    generation: u64,
    action_tx: tokio::sync::mpsc::Sender<Action>,
) {
    let Some(data) = session.data() else {
        let _ = action_tx
            .send(Action::ValueFetchFailed {
                generation,
                message: "data capability unavailable".into(),
            })
            .await;
        return;
    };
    match data.fetch_value(&value, offset, limit).await {
        Ok(bytes) => {
            let _ = action_tx
                .send(Action::ValueFetched { generation, bytes })
                .await;
        }
        Err(error) => {
            let _ = action_tx
                .send(Action::ValueFetchFailed {
                    generation,
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub fn table_request(
    object: QualifiedName,
    columns: Vec<dexo_driver_api::ColumnId>,
    filter: Option<Filter>,
    sort: Vec<Sort>,
    offset: u64,
    limit: u32,
    clauses: dexo_driver_api::RawClauses,
) -> Result<DataRequest, String> {
    Ok(DataRequest {
        clauses,
        object,
        columns,
        filter,
        sort,
        page: Page::new(offset, limit).map_err(|error| error.to_string())?,
    })
}
