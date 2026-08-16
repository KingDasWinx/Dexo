use std::sync::Arc;

use dexo_driver_api::{DataRequest, Filter, Page, QualifiedName, Session, Sort};

use crate::action::Action;
use crate::runtime::SessionId;

pub async fn fetch_page(
    session: Arc<dyn Session>,
    request: DataRequest,
    generation: u64,
    session_id: SessionId,
    action_tx: tokio::sync::mpsc::Sender<Action>,
) {
    let Some(data) = session.data() else {
        let _ = action_tx
            .send(Action::DataPageFailed {
                generation,
                message: "data capability unavailable".into(),
            })
            .await;
        return;
    };
    match data.fetch(request).await {
        Ok(page) => {
            let _ = action_tx
                .send(Action::DataPageLoaded {
                    generation,
                    session: session_id.0.to_string(),
                    page,
                })
                .await;
        }
        Err(error) => {
            let _ = action_tx
                .send(Action::DataPageFailed {
                    generation,
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
            .send(Action::DataPageFailed {
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
                .send(Action::DataPageFailed {
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
) -> Result<DataRequest, String> {
    Ok(DataRequest {
        object,
        columns,
        filter,
        sort,
        page: Page::new(offset, limit).map_err(|error| error.to_string())?,
    })
}
