use uuid::Uuid;

use crate::action::{
    DocumentIoRequest, PersistHistoryRequest, RecoveryCheckpointRequest, ScriptRequest,
};
use crate::Action;

pub mod session_registry;

pub use session_registry::SessionId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OperationId(pub Uuid);

impl OperationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationKey {
    pub operation: OperationId,
    pub session: String,
    pub document: String,
    pub generation: u64,
}

impl OperationKey {
    pub fn new(
        operation: OperationId,
        session: impl Into<String>,
        document: impl Into<String>,
        generation: u64,
    ) -> Self {
        Self {
            operation,
            session: session.into(),
            document: document.into(),
            generation,
        }
    }

    pub fn belongs_to(&self, session: &str, document: &str, generation: u64) -> bool {
        self.session == session && self.document == document && self.generation == generation
    }
}

#[allow(dead_code)]
pub struct WorkbenchRuntime {
    action_tx: tokio::sync::mpsc::Sender<Action>,
}

impl WorkbenchRuntime {
    pub fn new(action_tx: tokio::sync::mpsc::Sender<Action>) -> Self {
        Self { action_tx }
    }

    #[allow(dead_code)]
    pub async fn dispatch(&mut self, effect: crate::Effect) {
        match effect {
            crate::Effect::CreateConnection { input, password } => {
                self.create_connection(input, password).await
            }
            crate::Effect::ConnectProfile { profile } => self.connect_profile(profile).await,
            crate::Effect::StartScript(request) => self.start_script(request),
            crate::Effect::CancelOperation(id) => self.cancel_operation(id).await,
            crate::Effect::BeginTransaction { session, mode } => self.begin(session, mode).await,
            crate::Effect::CommitTransaction { session } => self.commit(session).await,
            crate::Effect::RollbackTransaction { session } => self.rollback(session).await,
            crate::Effect::Savepoint { session, name } => self.savepoint(session, name).await,
            crate::Effect::RollbackToSavepoint { session, name } => {
                self.rollback_to(session, name).await
            }
            crate::Effect::ReleaseSavepoint { session, name } => {
                self.release_savepoint(session, name).await
            }
            crate::Effect::LoadDocument(request) => self.load_document(request).await,
            crate::Effect::SaveDocument(request) => self.save_document(request).await,
            crate::Effect::CheckpointRecovery(request) => self.checkpoint_recovery(request).await,
            crate::Effect::PersistHistory(request) => self.persist_history(request).await,
            crate::Effect::PersistLayout => self.persist_layout().await,
            crate::Effect::Shutdown | crate::Effect::Quit => self.shutdown().await,
        }
    }

    async fn create_connection(&mut self, _input: dexo_app::NewConnection, _password: String) {}

    async fn connect_profile(&mut self, _profile: dexo_app::ConnectionProfile) {}

    fn start_script(&mut self, _request: ScriptRequest) {}

    async fn cancel_operation(&mut self, _id: OperationId) {}

    async fn begin(&mut self, _session: SessionId, _mode: dexo_driver_api::TransactionMode) {}

    async fn commit(&mut self, _session: SessionId) {}

    async fn rollback(&mut self, _session: SessionId) {}

    async fn savepoint(&mut self, _session: SessionId, _name: String) {}

    async fn rollback_to(&mut self, _session: SessionId, _name: String) {}

    async fn release_savepoint(&mut self, _session: SessionId, _name: String) {}

    async fn load_document(&mut self, _request: DocumentIoRequest) {}

    async fn save_document(&mut self, _request: DocumentIoRequest) {}

    async fn checkpoint_recovery(&mut self, _request: RecoveryCheckpointRequest) {}

    async fn persist_history(&mut self, _request: PersistHistoryRequest) {}

    async fn persist_layout(&mut self) {}

    async fn shutdown(&mut self) {}

    pub fn action_tx(&self) -> &tokio::sync::mpsc::Sender<Action> {
        &self.action_tx
    }
}
