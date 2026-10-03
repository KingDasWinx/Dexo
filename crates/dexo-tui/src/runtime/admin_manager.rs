use std::collections::HashMap;

use dexo_driver_api::{AdminList, SessionInfo};
use uuid::Uuid;

use crate::runtime::OperationId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdminView {
    Sessions,
    Locks,
    BlockingGraph,
    Statistics,
    Sizes,
    Variables,
}

pub struct AdminManager {
    selected: String,
    latest: HashMap<String, OperationId>,
    sessions: Vec<SessionInfo>,
}

impl AdminManager {
    pub fn new(selected: impl Into<String>) -> Self {
        Self {
            selected: selected.into(),
            latest: HashMap::new(),
            sessions: Vec::new(),
        }
    }

    pub fn refresh(&mut self, session: impl Into<String>, _view: AdminView) -> OperationId {
        let id = OperationId::new();
        self.latest.insert(session.into(), id);
        id
    }

    pub fn complete(&mut self, operation: OperationId, sessions: Vec<SessionInfo>) {
        if self
            .latest
            .get(&self.selected)
            .is_some_and(|current| *current == operation)
        {
            self.sessions = sessions;
        }
    }

    pub fn sessions(&self) -> &[SessionInfo] {
        &self.sessions
    }
}

pub async fn load_live(
    session: std::sync::Arc<dyn dexo_driver_api::Session>,
    tx: tokio::sync::mpsc::Sender<crate::action::Action>,
) {
    let Some(admin) = session.admin() else {
        let _ = tx
            .send(crate::action::Action::AdminFailed {
                message: "This connection has no session administration.".into(),
            })
            .await;
        return;
    };
    let sessions = match admin.list_sessions().await {
        Ok(list) => list,
        Err(error) => {
            let _ = tx
                .send(crate::action::Action::AdminFailed {
                    message: error.to_string(),
                })
                .await;
            return;
        }
    };
    let blocking = admin
        .blocking_graph()
        .await
        .map(|list| list.items)
        .unwrap_or_default();
    let _ = tx
        .send(crate::action::Action::AdminSessionsLoaded {
            sessions: sessions.items,
            captured_at: sessions.captured_at,
            blocking,
        })
        .await;
}

/// Reads `view` -- locks, sizes, statistics or settings -- off `session`.
pub async fn load_view(
    session: std::sync::Arc<dyn dexo_driver_api::Session>,
    through: crate::runtime::SessionId,
    view: crate::screens::admin::ServerView,
) -> crate::action::Action {
    use crate::screens::admin::{ServerView, ViewRows};
    let result = match session.admin() {
        None => Err("This connection has no server administration.".to_string()),
        Some(admin) => {
            let text = |error: dexo_driver_api::DriverError| error.to_string();
            match view {
                ServerView::Sessions => Err("Sessions are read on their own.".to_string()),
                ServerView::Locks => admin
                    .list_locks()
                    .await
                    .map(|list| (ViewRows::Locks(list.items), list.restriction))
                    .map_err(text),
                ServerView::Sizes => match dexo_driver_api::Page::new(0, 200) {
                    Ok(page) => admin
                        .sizes(page)
                        .await
                        .map(|list| (ViewRows::Sizes(list.items), list.restriction))
                        .map_err(text),
                    Err(error) => Err(error.to_string()),
                },
                ServerView::Stats => admin
                    .statistics()
                    .await
                    .map(|list| (ViewRows::Stats(list.items), list.restriction))
                    .map_err(text),
                ServerView::Settings => admin
                    .variables()
                    .await
                    .map(|list| (ViewRows::Settings(list.items), list.restriction))
                    .map_err(text),
            }
        }
    };
    crate::action::Action::AdminViewLoaded {
        session: through,
        view,
        result,
    }
}

/// Runs `act` -- a cancel or a terminate -- and says how it went.
pub async fn act_live(
    session: std::sync::Arc<dyn dexo_driver_api::Session>,
    act: dexo_driver_api::AdminAction,
    tx: tokio::sync::mpsc::Sender<crate::action::Action>,
) {
    let result = match session.admin() {
        Some(admin) => admin
            .execute_action(act.clone())
            .await
            .map(|outcome| outcome.message)
            .map_err(|error| error.to_string()),
        None => Err("this connection has no administration".into()),
    };
    let _ = tx.send(acted(&act, result)).await;
}

/// What became of `act`, as the screen hears it.
pub fn acted(
    act: &dexo_driver_api::AdminAction,
    result: Result<String, String>,
) -> crate::action::Action {
    match act {
        dexo_driver_api::AdminAction::CancelQuery { .. } => {
            crate::action::Action::AdminCancelled { result }
        }
        _ => crate::action::Action::AdminTerminated { result },
    }
}

pub fn session_info(id: &str) -> SessionInfo {
    SessionInfo {
        id: id.into(),
        user: None,
        database: None,
        state: "idle".into(),
        duration_ms: None,
        current_query: None,
        application: None,
        client: None,
    }
}

pub fn session_id(n: u128) -> String {
    Uuid::from_u128(n).to_string()
}

pub fn list_from(items: Vec<SessionInfo>) -> AdminList<SessionInfo> {
    AdminList {
        items,
        restriction: None,
        captured_at: "now".into(),
    }
}
