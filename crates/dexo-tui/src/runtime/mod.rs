use std::sync::Arc;

use dexo_app::{
    ConnectionProfile, DriverRegistry, NewConnection, QueryService, SecretPersist,
    create_connection, map_driver_error,
};
use dexo_runtime::TaskRegistry;
use dexo_secrets::{KeyringSecretStore, MemorySecretStore, SecretError, SecretStore};
use dexo_storage::{AppPaths, ConnectionRepository, Database};
use secrecy::SecretString;
use uuid::Uuid;

use crate::action::{
    Action, DocumentIoRequest, PersistHistoryRequest, RecoveryCheckpointRequest, ScriptRequest,
};

pub mod query_runner;
pub mod session_registry;
pub mod storage_worker;

pub use session_registry::SessionId;
use session_registry::SessionRegistry;
use storage_worker::StorageWorker;

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

#[derive(Default)]
pub(crate) struct SessionSecrets {
    keyring: KeyringSecretStore,
    memory: MemorySecretStore,
}

impl SecretStore for SessionSecrets {
    fn put(&self, key: &str, value: &str) -> Result<(), SecretError> {
        match self.keyring.put(key, value) {
            Ok(()) => {
                let _ = self.memory.put(key, value);
                Ok(())
            }
            Err(SecretError::Unavailable) => self.memory.put(key, value),
            Err(error) => Err(error),
        }
    }

    fn get(&self, key: &str) -> Result<Option<SecretString>, SecretError> {
        if let Ok(Some(secret)) = self.memory.get(key) {
            return Ok(Some(secret));
        }
        self.keyring.get(key)
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        let _ = self.memory.delete(key);
        self.keyring.delete(key)
    }
}

pub struct WorkbenchRuntime {
    action_tx: tokio::sync::mpsc::Sender<Action>,
    storage: Option<StorageWorker>,
    sessions: SessionRegistry,
    drivers: DriverRegistry,
    secrets: SessionSecrets,
    query: QueryService,
    live: Arc<tokio::sync::Mutex<Option<query_runner::LiveQuery>>>,
}

impl WorkbenchRuntime {
    pub fn new(
        action_tx: tokio::sync::mpsc::Sender<Action>,
        storage: StorageWorker,
        drivers: DriverRegistry,
    ) -> Self {
        Self {
            action_tx,
            storage: Some(storage),
            sessions: SessionRegistry::default(),
            drivers,
            secrets: SessionSecrets::default(),
            query: QueryService::new(Arc::new(TaskRegistry::default())),
            live: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub fn sessions(&self) -> &SessionRegistry {
        &self.sessions
    }

    pub fn sessions_mut(&mut self) -> &mut SessionRegistry {
        &mut self.sessions
    }

    pub async fn dispatch(&mut self, effect: crate::Effect) {
        match effect {
            crate::Effect::CreateConnection { input, password } => {
                self.create_connection(input, password).await
            }
            crate::Effect::ConnectProfile { profile } => self.connect_profile(profile).await,
            crate::Effect::StartScript(request) => {
                let _ = self.start_script(request).await;
            }
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
            crate::Effect::LoadHistory { connection_id } => self.load_history(connection_id).await,
            crate::Effect::ClearHistory { connection_id } => {
                self.clear_history(connection_id).await
            }
            crate::Effect::PersistLayout => self.persist_layout().await,
            crate::Effect::Shutdown | crate::Effect::Quit => self.shutdown().await,
        }
    }

    async fn emit(&self, action: Action) {
        let _ = self.action_tx.send(action).await;
    }

    async fn create_connection(&mut self, input: NewConnection, password: String) {
        match self.save_profile(input, &password) {
            Ok(profile) => self.connect_profile(profile).await,
            Err(message) => self.emit(Action::ConnectionFormError { message }).await,
        }
    }

    fn save_profile(
        &self,
        input: NewConnection,
        password: &str,
    ) -> Result<ConnectionProfile, String> {
        // ponytail: second rusqlite handle beside the storage worker; fold into StorageCommand when writes contend.
        let paths = AppPaths::discover().map_err(|error| error.to_string())?;
        let db = Database::open(&paths.database).map_err(|error| error.to_string())?;
        let repo = ConnectionRepository::new(db.connection());
        let (profile, persist) = create_connection(input, password, &self.secrets, &repo)
            .map_err(|error| error.to_string())?;
        if persist == SecretPersist::SessionOnly {
            self.secrets
                .memory
                .put(profile.secret_ref.as_str(), password)
                .map_err(|error| error.to_string())?;
        }
        Ok(profile)
    }

    async fn connect_profile(&mut self, profile: ConnectionProfile) {
        match self.open_session(&profile).await {
            Ok(session) => {
                let id = self.sessions.insert(profile.name.clone(), session);
                let generation = self.sessions.get(id).map(|active| active.generation).unwrap_or(1);
                self.emit(Action::ConnectionChanged {
                    name: profile.name,
                    ready: true,
                    environment: profile.environment,
                    session: Some(id),
                    generation,
                })
                .await;
            }
            Err(message) => self.emit(Action::ConnectionFormError { message }).await,
        }
    }

    async fn open_session(
        &self,
        profile: &ConnectionProfile,
    ) -> Result<Arc<dyn dexo_driver_api::Session>, String> {
        let secret = self
            .secrets
            .get(profile.secret_ref.as_str())
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "secret is missing for this connection".to_string())?;
        let factory = self
            .drivers
            .get(&profile.driver)
            .map_err(|error| error.to_string())?;
        let (connect, _) = profile
            .connect_request(secret)
            .map_err(|error| error.to_string())?;
        let boxed = factory
            .connect(connect)
            .await
            .map_err(map_driver_error)
            .map_err(|error| error.to_string())?;
        Ok(Arc::from(boxed))
    }

    pub async fn start_script(&mut self, request: ScriptRequest) -> anyhow::Result<()> {
        let Some(session) = self.session_for_key(&request.key) else {
            self.emit(Action::OperationFailed {
                key: request.key,
                message: "session is closed".into(),
            })
            .await;
            anyhow::bail!("session is closed");
        };
        let query = QueryService::new(Arc::clone(self.query.registry()));
        let action_tx = self.action_tx.clone();
        let live = Arc::clone(&self.live);
        tokio::spawn(async move {
            query_runner::run_script(query, session, request, action_tx, live).await;
        });
        Ok(())
    }

    pub async fn cancel(&mut self, id: OperationId) {
        query_runner::cancel_live(&self.query, &self.live, id).await;
        let _ = self
            .action_tx
            .send(Action::OperationCancelled(OperationKey::new(
                id, "", "", 0,
            )))
            .await;
    }

    async fn cancel_operation(&mut self, id: OperationId) {
        self.cancel(id).await;
    }

    fn session_for_key(&self, key: &OperationKey) -> Option<Arc<dyn dexo_driver_api::Session>> {
        if let Ok(uuid) = Uuid::parse_str(&key.session) {
            return self
                .sessions
                .get(SessionId(uuid))
                .map(|active| Arc::clone(&active.session));
        }
        self.sessions
            .find_by_connection(&key.session)
            .map(|active| Arc::clone(&active.session))
    }

    async fn begin(&mut self, session: SessionId, mode: dexo_driver_api::TransactionMode) {
        let result = self.sessions.begin(session, mode).await;
        self.tx_result(session, result).await;
    }

    async fn commit(&mut self, session: SessionId) {
        let result = self.sessions.commit(session).await;
        self.tx_result(session, result).await;
    }

    async fn rollback(&mut self, session: SessionId) {
        let result = self.sessions.rollback(session).await;
        self.tx_result(session, result).await;
    }

    async fn savepoint(&mut self, session: SessionId, name: String) {
        let result = self.sessions.savepoint(session, &name).await;
        self.tx_result(session, result).await;
    }

    async fn rollback_to(&mut self, session: SessionId, name: String) {
        let result = self.sessions.rollback_to(session, &name).await;
        self.tx_result(session, result).await;
    }

    async fn release_savepoint(&mut self, session: SessionId, name: String) {
        let result = self.sessions.release_savepoint(session, &name).await;
        self.tx_result(session, result).await;
    }

    async fn tx_result(
        &self,
        session: SessionId,
        result: Result<dexo_driver_api::TransactionState, String>,
    ) {
        match result {
            Ok(state) => {
                let generation = self
                    .sessions
                    .get(session)
                    .map(|active| active.generation)
                    .unwrap_or(0);
                self.emit(Action::TransactionChanged {
                    session,
                    generation,
                    state,
                })
                .await;
            }
            Err(message) => {
                self.emit(Action::OperationFailed {
                    key: OperationKey::new(OperationId::new(), session.0.to_string(), "", 0),
                    message,
                })
                .await;
            }
        }
    }

    async fn load_document(&mut self, _request: DocumentIoRequest) {}

    async fn save_document(&mut self, _request: DocumentIoRequest) {}

    async fn checkpoint_recovery(&mut self, _request: RecoveryCheckpointRequest) {}

    async fn persist_history(&mut self, request: PersistHistoryRequest) {
        if let Some(storage) = &self.storage {
            let _ = storage.persist_history(request.connection_id, request.sql);
        }
    }

    async fn load_history(&mut self, connection_id: Option<String>) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.list_history(connection_id).await {
            Ok(entries) => self.emit(Action::HistoryLoaded(entries)).await,
            Err(error) => {
                self.emit(Action::OperationFailed {
                    key: OperationKey::new(OperationId::new(), "", "", 0),
                    message: error.to_string(),
                })
                .await;
            }
        }
    }

    async fn clear_history(&mut self, connection_id: String) {
        if let Some(storage) = &self.storage {
            let _ = storage.clear_history(connection_id);
        }
    }

    async fn persist_layout(&mut self) {}

    async fn shutdown(&mut self) {
        if let Some(storage) = &self.storage {
            storage.shutdown();
        }
    }

    pub fn action_tx(&self) -> &tokio::sync::mpsc::Sender<Action> {
        &self.action_tx
    }
}
