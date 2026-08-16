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
            .send(crate::action::Action::OperationFailed {
                key: crate::runtime::OperationKey::new(
                    crate::runtime::OperationId::new(),
                    "",
                    "",
                    0,
                ),
                message: "admin unavailable".into(),
            })
            .await;
        return;
    };
    let sessions = match admin.list_sessions().await {
        Ok(list) => list,
        Err(error) => {
            let _ = tx
                .send(crate::action::Action::OperationFailed {
                    key: crate::runtime::OperationKey::new(
                        crate::runtime::OperationId::new(),
                        "",
                        "",
                        0,
                    ),
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

pub async fn terminate_live(
    session: std::sync::Arc<dyn dexo_driver_api::Session>,
    target: String,
    tx: tokio::sync::mpsc::Sender<crate::action::Action>,
) {
    let Some(admin) = session.admin() else {
        return;
    };
    let action = dexo_driver_api::AdminAction::TerminateSession {
        session_id: target.clone(),
    };
    match admin.execute_action(action).await {
        Ok(outcome) => {
            let _ = tx
                .send(crate::action::Action::OperationFailed {
                    key: crate::runtime::OperationKey::new(
                        crate::runtime::OperationId::new(),
                        "",
                        "",
                        0,
                    ),
                    message: outcome.message,
                })
                .await;
        }
        Err(error) => {
            let _ = tx
                .send(crate::action::Action::OperationFailed {
                    key: crate::runtime::OperationKey::new(
                        crate::runtime::OperationId::new(),
                        "",
                        "",
                        0,
                    ),
                    message: error.to_string(),
                })
                .await;
        }
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
