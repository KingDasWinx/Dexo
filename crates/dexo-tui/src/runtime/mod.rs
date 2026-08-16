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
    TransferRequest,
};

pub mod admin_manager;
pub mod catalog_manager;
pub mod clipboard;
pub mod connection_manager;
pub mod data_manager;
pub mod diagnostic_manager;
pub mod document_io;
pub mod explain_manager;
pub mod native_tool_manager;
pub mod project_manager;
pub mod query_runner;
pub mod recovery_manager;
pub mod result_spool;
pub mod schema_manager;
pub mod session_registry;
pub mod settings_manager;
pub mod storage_worker;
pub mod transfer_manager;

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

pub(crate) struct SessionSecrets {
    keyring: Box<dyn SecretStore>,
    memory: MemorySecretStore,
}

impl Default for SessionSecrets {
    fn default() -> Self {
        Self {
            keyring: Box::new(KeyringSecretStore),
            memory: MemorySecretStore::default(),
        }
    }
}

impl SessionSecrets {
    pub fn put_memory(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.memory.put(key, value)
    }

    pub fn put_keychain(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.keyring.put(key, value)
    }
}

impl SecretStore for SessionSecrets {
    fn put(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.keyring.put(key, value)
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
    transfer: transfer_manager::TransferManager,
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
            transfer: transfer_manager::TransferManager::default(),
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
            crate::Effect::ConnectProfile { profile, token } => {
                self.connect_profile(profile, token).await
            }
            crate::Effect::SubmitSecret {
                kind,
                profile,
                secret,
            } => self.submit_secret(kind, profile, secret).await,
            crate::Effect::DuplicateProfile { id } => self.duplicate_profile(id).await,
            crate::Effect::TestConnection { input, password } => {
                self.test_input(input, password).await
            }
            crate::Effect::TestSavedProfile { profile } => self.test_saved(profile).await,
            crate::Effect::SaveProfile { profile } => self.save_existing(profile).await,
            crate::Effect::DeleteProfile {
                profile,
                delete_secrets,
            } => self.delete_profile(profile, delete_secrets).await,
            crate::Effect::MoveProfileGroup { id, group_path } => {
                self.move_group(id, group_path).await
            }
            crate::Effect::CloseSession { session } => self.close_session(session).await,
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
            crate::Effect::PreviewDdl {
                change,
                session,
                generation: _,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    schema_manager::preview_live(
                        Arc::clone(&active.session),
                        session.0.to_string(),
                        change,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::ApplyDdlChange {
                change,
                typed,
                session,
                generation: _,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    schema_manager::apply_live(
                        Arc::clone(&active.session),
                        session.0.to_string(),
                        change,
                        typed,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::LoadSchemaDiff {
                session,
                left,
                right,
                generation: _,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    schema_manager::diff_live(
                        Arc::clone(&active.session),
                        session.0.to_string(),
                        left,
                        right,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::LoadSecurity {
                session,
                generation: _,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    Self::load_security_session(
                        Arc::clone(&active.session),
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::RunExplain {
                sql,
                analyze,
                session,
                generation: _,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    explain_manager::run_live(
                        Arc::clone(&active.session),
                        &sql,
                        0,
                        analyze,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::LoadAdminSessions { session, .. } => {
                if let Some(active) = self.sessions.get(session) {
                    admin_manager::load_live(Arc::clone(&active.session), self.action_tx.clone())
                        .await;
                }
            }
            crate::Effect::AdminTerminate { session, target } => {
                if let Some(active) = self.sessions.get(session) {
                    admin_manager::terminate_live(
                        Arc::clone(&active.session),
                        target,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::LoadMcpProfiles => self.load_mcp_profiles().await,
            crate::Effect::LoadConnectionProfiles => {
                match self.with_repo(|repo| repo.list().map_err(|error| error.to_string())) {
                    Ok(profiles) => self.emit(Action::ProfilesLoaded(profiles)).await,
                    Err(message) => self.emit(Action::ConnectionFormError { message }).await,
                }
            }
            crate::Effect::LoadMcpAudit => self.load_mcp_audit().await,
            crate::Effect::EnableMcpProfile { name } => self.enable_mcp_profile(name).await,
            crate::Effect::RevokeMcpGrants { profile } => self.revoke_mcp(profile).await,
            crate::Effect::RunTransfer(request) => self.dispatch_transfer(request).await,
            crate::Effect::LoadSnippets => {
                if let Some(storage) = &self.storage
                    && let Ok(snippets) = storage.list_snippets().await
                {
                    self.emit(Action::SnippetsLoaded(snippets)).await;
                }
            }
            crate::Effect::CheckpointRecovery(request) => self.checkpoint_recovery(request).await,
            crate::Effect::PersistHistory(request) => self.persist_history(request).await,
            crate::Effect::LoadHistory { connection_id } => self.load_history(connection_id).await,
            crate::Effect::ClearHistory { connection_id } => {
                self.clear_history(connection_id).await
            }
            crate::Effect::PersistLayout { project_id, layout } => {
                self.persist_layout(project_id, layout).await
            }
            crate::Effect::SwitchProject { name } => self.switch_project(name).await,
            crate::Effect::CreateProject { name } => self.create_project(name).await,
            crate::Effect::RenameProject { id, name } => self.rename_project(id, name).await,
            crate::Effect::DeleteProject {
                id,
                delete_connections,
            } => self.delete_project(id, delete_connections).await,
            crate::Effect::PreviewProjectDelete { id } => self.preview_delete(id).await,
            crate::Effect::LoadProject { id } => self.load_project(id).await,
            crate::Effect::ListProjects => self.list_projects().await,
            crate::Effect::ExportConfig { path } => self.export_config(path).await,
            crate::Effect::ImportConfig { path } => self.preview_import(path).await,
            crate::Effect::ApplyConfigImport { path, resolutions } => {
                self.apply_import(path, resolutions).await
            }
            crate::Effect::FlushDocuments {
                project_id,
                documents,
            } => self.flush_documents(project_id, documents).await,
            crate::Effect::CloseProjectSessions => self.close_project_sessions().await,
            crate::Effect::LoadCatalogChildren {
                parent,
                operation,
                session,
                generation,
                replace_roots,
                include_system,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    catalog_manager::load_children(
                        Arc::clone(&active.session),
                        parent,
                        operation,
                        session,
                        generation,
                        replace_roots,
                        include_system,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::LoadObjectInspector {
                id,
                session,
                generation,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    catalog_manager::load_inspector(
                        Arc::clone(&active.session),
                        id,
                        generation,
                        session,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::LoadTableData {
                request,
                session,
                generation,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    data_manager::fetch_page(
                        Arc::clone(&active.session),
                        request,
                        generation,
                        session,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::FetchValue {
                value,
                offset,
                limit,
                session,
                generation,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    data_manager::fetch_value(
                        Arc::clone(&active.session),
                        value,
                        offset,
                        limit,
                        generation,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::ApplyMutations {
                mutations,
                session,
                generation,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    data_manager::apply_mutations(
                        Arc::clone(&active.session),
                        mutations,
                        generation,
                        session,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::CopyToClipboard { text } => match clipboard::copy_text(text.clone()) {
                Ok(()) => self.emit(Action::ClipboardWritten { text }).await,
                Err(message) => self.emit(Action::ClipboardFailed { message }).await,
            },
            crate::Effect::CaptureCatalogSnapshot {
                connection_id,
                database_name,
                session,
                include_system,
            } => {
                if let Some(active) = self.sessions.get(session)
                    && let Ok(paths) = AppPaths::discover()
                {
                    catalog_manager::capture_snapshot(
                        Arc::clone(&active.session),
                        connection_id,
                        database_name,
                        include_system,
                        paths.database,
                    )
                    .await;
                }
            }
            crate::Effect::LoadOfflineCatalog {
                connection_id,
                database_name,
                generation,
            } => {
                self.load_offline_catalog(connection_id, database_name, generation)
                    .await
            }
            crate::Effect::LoadObjectUsage {
                project_id,
                connection_id,
            } => self.load_object_usage(project_id, connection_id).await,
            crate::Effect::PersistFavorite {
                project_id,
                connection_id,
                object_id,
                favorite,
            } => self.persist_favorite(project_id, connection_id, object_id, favorite),
            crate::Effect::Shutdown | crate::Effect::Quit => self.shutdown().await,
        }
    }

    async fn dispatch_transfer(&mut self, request: TransferRequest) {
        let operation = request.operation();
        let access = match &request {
            TransferRequest::Export { .. } => transfer_manager::RuntimeAccess {
                action_tx: self.action_tx.clone(),
                session: None,
                driver: None,
                host: None,
                port: None,
                database: None,
                username: None,
                secret: None,
            },
            TransferRequest::Import { session, .. }
            | TransferRequest::Backup { session, .. }
            | TransferRequest::Restore { session, .. } => match self.transfer_access(*session) {
                Ok(access) => access,
                Err(message) => {
                    self.emit(Action::TransferFailed { operation, message })
                        .await;
                    return;
                }
            },
        };
        let _ = self.transfer.run_with(request, Some(&access)).await;
    }

    fn transfer_access(
        &self,
        session: SessionId,
    ) -> Result<transfer_manager::RuntimeAccess, String> {
        let active = self
            .sessions
            .get(session)
            .ok_or_else(|| "session is closed".to_string())?;
        let session_arc = Arc::clone(&active.session);
        let name = active.connection.clone();
        let profile = self.with_repo(|repo| {
            repo.list()
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|profile| profile.name == name)
                .ok_or_else(|| "connection profile not found".into())
        })?;
        let secret = self
            .secrets
            .get(profile.secret_ref.as_str())
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "secret is missing for this connection".to_string())?;
        transfer_manager::RuntimeAccess::from_profile(
            self.action_tx.clone(),
            session_arc,
            &profile,
            secret,
        )
    }

    async fn emit(&self, action: Action) {
        let _ = self.action_tx.send(action).await;
    }

    async fn load_security_session(
        session: Arc<dyn dexo_driver_api::Session>,
        tx: tokio::sync::mpsc::Sender<Action>,
    ) {
        let Some(admin) = session.security() else {
            let _ = tx
                .send(Action::SecurityFailed {
                    message: "this driver does not offer security admin".into(),
                })
                .await;
            return;
        };
        match admin.list_grants(None).await {
            Ok(grants) => {
                let mut principals: Vec<String> = grants
                    .iter()
                    .map(|grant| grant.principal.object().to_string())
                    .collect();
                principals.sort();
                principals.dedup();
                let _ = tx.send(Action::SecurityLoaded { principals, grants }).await;
            }
            Err(error) => {
                let _ = tx
                    .send(Action::SecurityFailed {
                        message: error.to_string(),
                    })
                    .await;
            }
        }
    }

    async fn create_connection(&mut self, input: NewConnection, password: String) {
        match self.save_profile(input, &password) {
            Ok((profile, SecretPersist::Stored)) => {
                self.emit(Action::ProfileSaved(profile.clone())).await;
                self.connect_profile(profile, 0).await;
            }
            Ok((profile, SecretPersist::SessionOnly)) => {
                self.emit(Action::SecretRequired {
                    purpose: crate::screens::secret_prompt::SecretPurpose::DatabasePassword,
                    profile,
                    buffer: crate::screens::secret_prompt::SecretBuffer::new(password),
                })
                .await;
            }
            Err(message) => self.emit(Action::ConnectionFormError { message }).await,
        }
    }

    fn save_profile(
        &self,
        input: NewConnection,
        password: &str,
    ) -> Result<(ConnectionProfile, SecretPersist), String> {
        // ponytail: second rusqlite handle beside the storage worker; fold into StorageCommand when writes contend.
        self.with_repo(|repo| {
            create_connection(input, password, &self.secrets, repo)
                .map_err(|error| error.to_string())
        })
    }

    async fn submit_secret(
        &mut self,
        kind: crate::screens::secret_prompt::SecretChoiceKind,
        profile: ConnectionProfile,
        secret: crate::screens::secret_prompt::SecretBuffer,
    ) {
        use crate::screens::secret_prompt::SecretChoiceKind;
        let key = profile.secret_ref.as_str();
        let result = match kind {
            SecretChoiceKind::Cancel => return,
            SecretChoiceKind::SessionOnly => self.secrets.put_memory(key, secret.expose()),
            SecretChoiceKind::SaveToKeychain => self.secrets.put_keychain(key, secret.expose()),
        };
        match result {
            Ok(()) => self.connect_profile(profile, 0).await,
            Err(SecretError::Unavailable) => {
                self.emit(Action::SecretRequired {
                    purpose: crate::screens::secret_prompt::SecretPurpose::DatabasePassword,
                    profile,
                    buffer: secret,
                })
                .await;
            }
            Err(error) => {
                self.emit(Action::ConnectionFormError {
                    message: error.to_string(),
                })
                .await;
            }
        }
    }

    async fn connect_profile(&mut self, profile: ConnectionProfile, token: u64) {
        match connection_manager::ConnectionManager::new(&self.secrets).connect(&profile) {
            Err(action) => self.emit(*action).await,
            Ok(_) => match self.open_session(&profile).await {
                Ok(session) => {
                    let id = self.sessions.insert(profile.name.clone(), session);
                    let generation = self
                        .sessions
                        .get(id)
                        .map(|active| active.generation)
                        .unwrap_or(1);
                    let read_only =
                        dexo_app::ConnectionPolicy::resolve(&profile.environment, &profile.policy)
                            .map(|policy| policy.read_only)
                            .unwrap_or(false);
                    self.emit(Action::ConnectionChanged {
                        name: profile.name,
                        ready: true,
                        environment: profile.environment,
                        session: Some(id),
                        generation,
                        token,
                        read_only,
                    })
                    .await;
                }
                Err(message) => self.emit(Action::ConnectionFormError { message }).await,
            },
        }
    }

    async fn duplicate_profile(&mut self, id: dexo_app::ConnectionId) {
        match self.with_repo(|repo| repo.duplicate(id).map_err(|error| error.to_string())) {
            Ok(profile) => self.emit(Action::ProfileSaved(profile)).await,
            Err(message) => self.emit(Action::ConnectionFormError { message }).await,
        }
    }

    async fn save_existing(&mut self, profile: ConnectionProfile) {
        match self.with_repo(|repo| repo.update(&profile).map_err(|error| error.to_string())) {
            Ok(()) => self.emit(Action::ProfileSaved(profile)).await,
            Err(message) => self.emit(Action::ConnectionFormError { message }).await,
        }
    }

    async fn move_group(&mut self, id: dexo_app::ConnectionId, group_path: Option<String>) {
        match self.with_repo(|repo| {
            repo.move_group(id, group_path.as_deref())
                .map_err(|error| error.to_string())?;
            repo.list().map_err(|error| error.to_string())
        }) {
            Ok(profiles) => self.emit(Action::ProfilesLoaded(profiles)).await,
            Err(message) => self.emit(Action::ConnectionFormError { message }).await,
        }
    }

    async fn delete_profile(&mut self, profile: ConnectionProfile, delete_secrets: bool) {
        if delete_secrets {
            match self.secrets.delete(profile.secret_ref.as_str()) {
                Ok(()) => {}
                Err(SecretError::Unavailable) | Err(SecretError::Internal) => {
                    self.emit(Action::ConnectionFormError {
                        message: format!(
                            "keychain delete failed for {}; choose keep secrets to remove the profile only",
                            profile.name
                        ),
                    })
                    .await;
                    return;
                }
            }
        }
        match self.with_repo(|repo| repo.delete(profile.id).map_err(|error| error.to_string())) {
            Ok(()) => {
                self.emit(Action::ProfileDeleted { name: profile.name })
                    .await;
            }
            Err(message) => self.emit(Action::ConnectionFormError { message }).await,
        }
    }

    async fn test_input(&mut self, input: NewConnection, password: String) {
        match dexo_app::test_connection_input(input) {
            Ok(profile) => {
                if let Err(error) = self
                    .secrets
                    .put_memory(profile.secret_ref.as_str(), &password)
                {
                    self.emit(Action::ConnectionFormError {
                        message: error.to_string(),
                    })
                    .await;
                    return;
                }
                let name = profile.name.clone();
                match self.open_session(&profile).await {
                    Ok(_) => {
                        self.emit(Action::ConnectionTested {
                            name,
                            ok: true,
                            message: "ok".into(),
                        })
                        .await;
                    }
                    Err(message) => {
                        self.emit(Action::ConnectionTested {
                            name,
                            ok: false,
                            message,
                        })
                        .await;
                    }
                }
            }
            Err(error) => {
                self.emit(Action::ConnectionFormError {
                    message: error.to_string(),
                })
                .await;
            }
        }
    }

    async fn test_saved(&mut self, profile: ConnectionProfile) {
        let name = profile.name.clone();
        let (ok, message) = match self.open_session(&profile).await {
            Ok(_) => (true, "ok".into()),
            Err(message) => (false, message),
        };
        self.emit(Action::ConnectionTested { name, ok, message })
            .await;
    }

    async fn close_session(&mut self, session: SessionId) {
        self.sessions.remove(session);
        self.emit(Action::SessionClosed { session }).await;
    }

    fn with_repo<T>(
        &self,
        f: impl FnOnce(&ConnectionRepository<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        // ponytail: second rusqlite handle beside the storage worker; fold into StorageCommand when writes contend.
        let paths = AppPaths::discover().map_err(|error| error.to_string())?;
        let db = Database::open(&paths.database).map_err(|error| error.to_string())?;
        f(&ConnectionRepository::new(db.connection()))
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
        if self.transfer.cancel(id).await {
            let _ = self
                .action_tx
                .send(Action::OperationCancelled(OperationKey::new(id, "", "", 0)))
                .await;
            return;
        }
        query_runner::cancel_live(&self.query, &self.live, id).await;
        let _ = self
            .action_tx
            .send(Action::OperationCancelled(OperationKey::new(id, "", "", 0)))
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

    async fn load_document(&mut self, request: DocumentIoRequest) {
        match tokio::fs::read_to_string(&request.path).await {
            Ok(content) => {
                self.emit(Action::DocumentLoaded {
                    document: request.document,
                    content,
                })
                .await;
            }
            Err(error) => {
                self.emit(Action::OperationFailed {
                    key: OperationKey::new(OperationId::new(), "", request.document, 0),
                    message: error.to_string(),
                })
                .await;
            }
        }
    }

    async fn save_document(&mut self, request: DocumentIoRequest) {
        let result = if let Some(expected) = request.expected_fingerprint.as_deref() {
            match document_io::fingerprint(&request.path).await {
                Ok(disk) if disk.hash != expected => {
                    self.emit(Action::DocumentConflict {
                        path: request.path.display().to_string(),
                    })
                    .await;
                    return;
                }
                Ok(_) => document_io::save_sql_atomic(&request.path, &request.content).await,
                Err(error) => Err(error),
            }
        } else {
            document_io::save_sql_atomic(&request.path, &request.content).await
        };
        match result {
            Ok(()) => {
                if let Some(storage) = &self.storage {
                    let _ = storage.save_document(request);
                }
            }
            Err(document_io::DocumentIoError::ExternalConflict { path, .. }) => {
                self.emit(Action::DocumentConflict {
                    path: path.display().to_string(),
                })
                .await;
            }
            Err(error) => {
                self.emit(Action::OperationFailed {
                    key: OperationKey::new(OperationId::new(), "", String::new(), 0),
                    message: error.to_string(),
                })
                .await;
            }
        }
    }

    async fn checkpoint_recovery(&mut self, request: RecoveryCheckpointRequest) {
        if let Some(storage) = &self.storage {
            let _ = storage.checkpoint_recovery(request);
        }
    }

    async fn persist_layout(&mut self, project_id: String, layout: dexo_storage::WorkbenchLayout) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.persist_layout_wait(project_id, layout).await {
            Ok(()) => self.emit(Action::LayoutPersisted).await,
            Err(error) => {
                self.emit(Action::ProjectSwitchFailed {
                    message: error.to_string(),
                })
                .await;
            }
        }
    }

    async fn flush_documents(
        &mut self,
        project_id: String,
        documents: Vec<crate::action::FlushedDocument>,
    ) {
        let Some(storage) = &self.storage else {
            self.emit(Action::DocumentsFlushed).await;
            return;
        };
        match storage.flush_documents(project_id, documents).await {
            Ok(()) => self.emit(Action::DocumentsFlushed).await,
            Err(error) => {
                self.emit(Action::ProjectSwitchFailed {
                    message: error.to_string(),
                })
                .await;
            }
        }
    }

    async fn list_projects(&mut self) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.list_projects().await {
            Ok(projects) => self.emit(Action::ProjectsLoaded(projects)).await,
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn create_project(&mut self, name: String) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.create_project(name).await {
            Ok(projects) => self.emit(Action::ProjectsLoaded(projects)).await,
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn rename_project(&mut self, id: String, name: String) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.rename_project(id, name).await {
            Ok(projects) => self.emit(Action::ProjectsLoaded(projects)).await,
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn delete_project(&mut self, id: String, delete_connections: bool) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.delete_project(id, delete_connections).await {
            Ok(name) => {
                self.emit(Action::ProjectDeleted { name }).await;
                if let Ok(projects) = storage.list_projects().await {
                    self.emit(Action::ProjectsLoaded(projects)).await;
                }
            }
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn preview_delete(&mut self, id: String) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.preview_delete(id).await {
            Ok((project, preview)) => {
                self.emit(Action::ProjectDeletePreviewed { project, preview })
                    .await;
            }
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn switch_project(&mut self, name: String) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.get_project_by_name(name.clone()).await {
            Ok(Some(project)) => {
                self.emit(Action::ProjectSwitchTarget(project)).await;
            }
            Ok(None) => {
                self.fail_project(anyhow::anyhow!("unknown project {name}"))
                    .await;
            }
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn load_project(&mut self, id: String) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.load_project(id).await {
            Ok(loaded) => {
                self.emit(Action::ProjectLoaded {
                    project: loaded.project,
                    documents: loaded
                        .documents
                        .into_iter()
                        .map(|document| (document.id, document.content))
                        .collect(),
                    layout: loaded.layout,
                })
                .await;
            }
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn close_project_sessions(&mut self) {
        for id in self.sessions.ids() {
            self.sessions.remove(id);
            self.emit(Action::SessionClosed { session: id }).await;
        }
        self.emit(Action::ProjectSessionsClosed).await;
    }

    async fn export_config(&mut self, path: std::path::PathBuf) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.export_config(path).await {
            Ok(()) => {
                self.emit(Action::ConfigImported {
                    needing_secret: Vec::new(),
                })
                .await;
            }
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn preview_import(&mut self, path: std::path::PathBuf) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.preview_import(path).await {
            Ok(preview) => {
                self.emit(Action::ConfigPreviewed {
                    conflicts: preview.conflicts,
                    needing_secret: preview.connections_needing_secret,
                })
                .await;
            }
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn apply_import(
        &mut self,
        path: std::path::PathBuf,
        resolutions: std::collections::HashMap<String, dexo_storage::ImportResolution>,
    ) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.apply_import(path, resolutions).await {
            Ok(report) => {
                self.emit(Action::ConfigImported {
                    needing_secret: report.connections_needing_secret,
                })
                .await;
            }
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn fail_project(&self, error: impl ToString) {
        self.emit(Action::ProjectSwitchFailed {
            message: error.to_string(),
        })
        .await;
    }

    async fn persist_history(&mut self, request: PersistHistoryRequest) {
        if let Some(storage) = &self.storage {
            let _ = storage.persist_history(request.project_id, request.connection_id, request.sql);
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

    async fn shutdown(&mut self) {
        if let Some(storage) = &self.storage {
            let _ = storage.mark_clean_shutdown();
            storage.shutdown();
        }
    }

    async fn load_offline_catalog(
        &self,
        connection_id: String,
        database_name: String,
        generation: u64,
    ) {
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(db) = Database::open(&paths.database) else {
            return;
        };
        let cache = dexo_storage::CatalogCache::new(db.connection());
        let created_at = cache
            .latest_metadata(&connection_id, &database_name)
            .ok()
            .flatten()
            .map(|meta| meta.created_at);
        let Ok(objects) = cache.load_latest(&connection_id, &database_name) else {
            return;
        };
        self.emit(Action::OfflineCatalogLoaded {
            generation,
            list: dexo_driver_api::CatalogList {
                objects,
                restrictions: vec![],
            },
            created_at,
        })
        .await;
    }

    async fn load_object_usage(&self, project_id: String, connection_id: String) {
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(db) = Database::open(&paths.database) else {
            return;
        };
        let Ok(rows) = dexo_storage::ObjectUsageRepository::new(db.connection())
            .list_for_connection(&project_id, &connection_id)
        else {
            return;
        };
        let ids = rows
            .into_iter()
            .filter(|row| row.favorite)
            .map(|row| row.object_id)
            .collect();
        self.emit(Action::ApplyFavorites { ids }).await;
    }

    fn persist_favorite(
        &self,
        project_id: String,
        connection_id: String,
        object_id: String,
        favorite: bool,
    ) {
        // ponytail: second rusqlite handle; fold into StorageCommand if catalog writes contend.
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(db) = Database::open(&paths.database) else {
            return;
        };
        let _ = dexo_storage::ObjectUsageRepository::new(db.connection()).set_favorite(
            &project_id,
            &connection_id,
            &object_id,
            favorite,
        );
    }

    async fn load_mcp_profiles(&self) {
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(db) = Database::open(&paths.database) else {
            return;
        };
        let Ok(profiles) = dexo_storage::McpProfileRepository::new(db.connection()).list() else {
            return;
        };
        let Some(profile) = profiles.into_iter().next() else {
            self.emit(Action::McpProfilesLoaded {
                name: String::new(),
                enabled: false,
                scopes: Vec::new(),
                tools: Vec::new(),
            })
            .await;
            return;
        };
        self.emit(Action::McpProfilesLoaded {
            name: profile.name,
            enabled: profile.enabled,
            scopes: profile
                .selectors
                .iter()
                .map(|rule| format!("{rule:?}"))
                .collect(),
            tools: profile
                .tool_rules
                .iter()
                .map(|rule| rule.tool.clone())
                .collect(),
        })
        .await;
    }

    async fn load_mcp_audit(&self) {
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(ledger) = dexo_storage::SqliteGrantLedger::open(&paths.database) else {
            return;
        };
        use dexo_app::mcp::GrantLedger;
        let events = ledger
            .audits()
            .into_iter()
            .map(|event| format!("{} {} {}", event.profile, event.decision, event.target))
            .collect();
        self.emit(Action::McpAuditLoaded { events }).await;
    }

    async fn enable_mcp_profile(&self, name: String) {
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(db) = Database::open(&paths.database) else {
            return;
        };
        let repo = dexo_storage::McpProfileRepository::new(db.connection());
        if let Ok(Some(mut profile)) = repo.get_by_name(&name) {
            profile.enabled = true;
            let _ = repo.save(&profile);
        }
        self.load_mcp_profiles().await;
    }

    async fn revoke_mcp(&self, profile: String) {
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(ledger) = dexo_storage::SqliteGrantLedger::open(&paths.database) else {
            return;
        };
        use dexo_app::mcp::GrantLedger;
        let _ = ledger.revoke_profile(&profile);
        self.load_mcp_audit().await;
    }

    pub fn action_tx(&self) -> &tokio::sync::mpsc::Sender<Action> {
        &self.action_tx
    }
}
