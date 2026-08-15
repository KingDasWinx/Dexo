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
