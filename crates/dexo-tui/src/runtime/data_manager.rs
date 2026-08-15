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
