use serde::{Deserialize, Serialize};

use crate::{DriverError, LockLevel, Page, QualifiedName};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub user: Option<String>,
    pub database: Option<String>,
    pub state: String,
    pub duration_ms: Option<u64>,
    pub current_query: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LockInfo {
    pub lock_type: String,
    pub relation: Option<String>,
    pub mode: String,
    pub granted: bool,
    pub session_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockingEdge {
    pub blocker: String,
    pub blocked: String,
    pub lock: LockInfo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SizeInfo {
    pub object: String,
    pub native_size: Option<String>,
    pub bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatInfo {
    pub name: String,
    pub value: Option<String>,
    pub captured_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum VariableScope {
    Session,
    Server,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VariableInfo {
    pub name: String,
    pub value: Option<String>,
    pub scope: VariableScope,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminList<T> {
    pub items: Vec<T>,
    pub restriction: Option<String>,
    pub captured_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdminAction {
    CancelQuery { session_id: String },
    TerminateSession { session_id: String },
    Vacuum { target: QualifiedName },
    Analyze { target: QualifiedName },
    Reindex { target: QualifiedName },
    Optimize { target: QualifiedName },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AdminConfirmKind {
    Once,
    TypeTarget,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminPreview {
    pub command: String,
    pub lock_risk: LockLevel,
    pub confirmation: AdminConfirmKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminOutcome {
    pub ok: bool,
    pub idempotent_noop: bool,
    pub message: String,
}

#[async_trait::async_trait]
pub trait AdministrationProvider: Send + Sync {
    async fn list_sessions(&self) -> Result<AdminList<SessionInfo>, DriverError>;
    async fn list_locks(&self) -> Result<AdminList<LockInfo>, DriverError>;
    async fn blocking_graph(&self) -> Result<AdminList<BlockingEdge>, DriverError>;
    async fn sizes(&self, page: Page) -> Result<AdminList<SizeInfo>, DriverError>;
    async fn statistics(&self) -> Result<AdminList<StatInfo>, DriverError>;
    async fn variables(&self) -> Result<AdminList<VariableInfo>, DriverError>;
    fn preview(&self, action: &AdminAction) -> Result<AdminPreview, DriverError>;
    async fn execute_action(&self, action: AdminAction) -> Result<AdminOutcome, DriverError>;
}
