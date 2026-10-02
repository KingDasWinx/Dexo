use std::sync::Arc;
use std::time::Duration;

use dexo_app::{
    ConnectionProfile, DriverRegistry, NewConnection, QueryService, SecretPersist,
    create_connection, map_driver_error,
};
use dexo_runtime::TaskRegistry;
use dexo_secrets::{KeyringSecretStore, MemorySecretStore, SecretError, SecretStore};
use dexo_storage::{AppPaths, ConnectionRepository, Database};
use secrecy::{ExposeSecret, SecretString};
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
pub mod update_check;

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
    /// Shared with the spawned dials, so one the server turned down can forget it.
    memory: Arc<MemorySecretStore>,
}

impl Default for SessionSecrets {
    fn default() -> Self {
        Self {
            keyring: Box::new(KeyringSecretStore),
            memory: Arc::new(MemorySecretStore::default()),
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

/// How long a driver gets to answer before the connect is called a failure. An
/// unroutable host otherwise leaves the dial hanging for the OS timeout, and the user
/// sees a connection that never resolves either way.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A connect that finished, waiting for `Effect::AdoptSession` to move it into the
/// registry. Parked rather than returned because the dialling runs in its own task.
/// A dialled session waiting to be adopted, with what a password command printed for
/// it, kept in memory for the side connections (export, import) the session opens.
type OpenedSession = (
    u64,
    ConnectionProfile,
    Arc<dyn dexo_driver_api::Session>,
    Option<SecretString>,
);

/// Where a connection's password comes from, settled before anything slow runs.
enum Password {
    Ready(SecretString),
    /// A password manager's command: it may take seconds or hang, so it runs on a
    /// blocking thread inside the spawned dial, never on the event loop.
    Command(String),
}

impl Password {
    fn for_profile(
        profile: &ConnectionProfile,
        secrets: &SessionSecrets,
    ) -> Result<Self, Box<Action>> {
        match profile.password_command() {
            Some(command) => Ok(Self::Command(command.to_string())),
            None => connection_manager::ConnectionManager::new(secrets)
                .connect(profile)
                .map(Self::Ready),
        }
    }

    async fn resolve(self) -> Result<SecretString, String> {
        match self {
            Self::Ready(secret) => Ok(secret),
            Self::Command(command) => tokio::task::spawn_blocking(move || {
                // The workbench owns the terminal: a command that would ask there must
                // not share the keyboard with it.
                dexo_app::password_command::run_without_terminal(
                    &command,
                    dexo_app::password_command::TIMEOUT,
                )
            })
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string()),
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// `COUNT(*)`'s answer, however the driver types it.
fn count_of(value: &dexo_driver_api::DbValue) -> Option<u64> {
    match value {
        dexo_driver_api::DbValue::I64(rows) => u64::try_from(*rows).ok(),
        dexo_driver_api::DbValue::U64(rows) => Some(*rows),
        dexo_driver_api::DbValue::Decimal(rows) | dexo_driver_api::DbValue::Text(rows) => {
            rows.parse().ok()
        }
        _ => None,
    }
}

/// Dials `profile` with the password `password` resolves to, within `CONNECT_TIMEOUT`.
/// On a connect (`forget`), a password the server turns down is forgotten if this
/// session was holding it in memory -- a URL's, or one typed for the session -- so the
/// next connect asks again instead of failing with it for ever. A password command's
/// answer is not one, and a test only reports.
async fn dial(
    factory: Arc<dyn dexo_driver_api::ConnectionFactory>,
    profile: &ConnectionProfile,
    password: Password,
    memory: &MemorySecretStore,
    forget: bool,
) -> Result<
    (
        Box<dyn dexo_driver_api::Session>,
        SecretString,
        ConnectionProfile,
    ),
    String,
> {
    let forget = forget && matches!(password, Password::Ready(_));
    let secret = password.resolve().await?;
    // A pre-connect command opens the way first; the session keeps it running, and the
    // profile it hands back dials that way, for the connections that follow.
    let opened = dexo_app::connect::open(
        factory.as_ref(),
        profile,
        SecretString::from(secret.expose_secret().to_string()),
        Some(CONNECT_TIMEOUT),
    )
    .await;
    match opened {
        Ok(opened) => Ok((opened.session, secret, opened.profile)),
        Err(
            dexo_app::connect::ConnectError::PreConnect(error)
            | dexo_app::connect::ConnectError::Setup(error),
        ) => Err(error.to_string()),
        Err(dexo_app::connect::ConnectError::Driver(error)) => {
            let rejected = error.category() == dexo_driver_api::DriverErrorCategory::Authentication;
            let message = map_driver_error(error).to_string();
            let key = profile.secret_ref.as_str();
            if forget && rejected && matches!(memory.get(key), Ok(Some(_))) {
                let _ = memory.delete(key);
                return Err(format!("{message} -- connect again to enter the password"));
            }
            Err(message)
        }
        Err(dexo_app::connect::ConnectError::TimedOut(limit)) => Err(format!(
            "{} did not answer within {}s",
            profile.name,
            limit.as_secs()
        )),
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
    opening: Arc<tokio::sync::Mutex<Option<OpenedSession>>>,
    /// The profile each session was opened with. A temporary connection has no other
    /// record of one, and the side connections export and import open need it.
    session_profiles: std::collections::HashMap<SessionId, ConnectionProfile>,
    transfer: transfer_manager::TransferManager,
    /// The row counts running, each on a connection of its own.
    counts: Arc<std::sync::Mutex<std::collections::HashMap<OperationId, CountTask>>>,
}

/// A count's task, and once it has dialled, its connection and query: what a cancel
/// stops on the server.
#[derive(Default)]
struct CountTask {
    task: Option<tokio::task::AbortHandle>,
    running: Option<(Arc<dyn dexo_driver_api::Session>, dexo_driver_api::QueryId)>,
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
            opening: Arc::new(tokio::sync::Mutex::new(None)),
            session_profiles: std::collections::HashMap::new(),
            transfer: transfer_manager::TransferManager::default(),
            counts: Arc::default(),
        }
    }

    pub fn sessions(&self) -> &SessionRegistry {
        &self.sessions
    }

    /// Where `dexo <url>` puts the URL's password: this process's memory, nowhere else.
    pub fn remember_secret(&self, key: &str, secret: &SecretString) -> Result<(), SecretError> {
        self.secrets.put_memory(key, secret.expose_secret())
    }

    pub fn sessions_mut(&mut self) -> &mut SessionRegistry {
        &mut self.sessions
    }

    pub async fn dispatch(&mut self, effect: crate::Effect) {
        match effect {
            crate::Effect::CreateConnection {
                input,
                password,
                connect,
            } => self.create_connection(input, password, connect).await,
            crate::Effect::RenameSessions { from, to } => self.sessions.rename(&from, &to),
            crate::Effect::RevealTemporarySecret { profile } => {
                let password = self
                    .secrets
                    .memory
                    .get(profile.secret_ref.as_str())
                    .ok()
                    .flatten()
                    .map(|secret| {
                        crate::screens::secret_prompt::SecretBuffer::new(
                            secret.expose_secret().to_string(),
                        )
                    });
                self.emit(Action::TemporarySaveForm { profile, password })
                    .await;
            }
            crate::Effect::ConnectProfile { profile, token } => {
                self.connect_profile(profile, token).await
            }
            crate::Effect::AdoptSession { token } => self.adopt_session(token).await,
            crate::Effect::SubmitSecret {
                kind,
                profile,
                secret,
                token,
            } => self.submit_secret(kind, profile, secret, token).await,
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
            crate::Effect::CountRows {
                session,
                operation,
                sql,
                parameters,
            } => self.count_rows(session, operation, sql, parameters).await,
            crate::Effect::LoadNote {
                connection_id,
                object,
            } => {
                let action_tx = self.action_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let note = AppPaths::discover()
                        .ok()
                        .and_then(|paths| Database::open(&paths.database).ok())
                        .and_then(|db| {
                            dexo_storage::ObjectNoteRepository::new(db.connection())
                                .get(&connection_id, &object)
                                .ok()
                                .flatten()
                        });
                    let _ = action_tx.blocking_send(Action::NoteLoaded { object, note });
                });
            }
            crate::Effect::SaveNote {
                connection_id,
                object,
                note,
            } => {
                let action_tx = self.action_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let saved = AppPaths::discover()
                        .map_err(|error| error.to_string())
                        .and_then(|paths| {
                            Database::open(&paths.database).map_err(|error| error.to_string())
                        })
                        .and_then(|db| {
                            dexo_storage::ObjectNoteRepository::new(db.connection())
                                .set(&connection_id, &object, &note)
                                .map_err(|error| error.to_string())
                        });
                    let message = match saved {
                        Ok(()) => format!("Saved the note on {object}."),
                        Err(error) => format!("The note was not saved: {error}"),
                    };
                    let _ = action_tx.blocking_send(Action::Notice(message));
                });
            }
            crate::Effect::DiscoverDocker => {
                let action_tx = self.action_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let found = dexo_app::docker::discover(Duration::from_secs(3));
                    let _ = action_tx.blocking_send(Action::DockerDiscovered(found));
                });
            }
            crate::Effect::SaveQuery {
                project_id,
                connection_id,
                name,
                sql,
            } => {
                let Some(storage) = &self.storage else {
                    return;
                };
                let done = storage
                    .save_query(project_id, connection_id, name, sql)
                    .await
                    .map(|(saved, replaced)| match replaced {
                        true => format!(
                            "Saved query {}, replacing the one of that name.",
                            saved.name
                        ),
                        false => format!("Saved query {}.", saved.name),
                    })
                    .map_err(|error| format!("The query was not saved: {error}"));
                self.emit(Action::SavedQueryDone(done)).await;
            }
            crate::Effect::LoadSavedQueries { project_id } => {
                self.list_saved_queries(project_id).await;
            }
            crate::Effect::RenameSavedQuery {
                project_id,
                id,
                name,
            } => {
                let Some(storage) = &self.storage else {
                    return;
                };
                let done = storage
                    .rename_saved_query(project_id.clone(), id, name.clone())
                    .await
                    .map(|()| format!("Renamed to {name}."))
                    .map_err(|error| error.to_string());
                // A refused name stays in the field to be fixed; the list is read again
                // only once the rename went through.
                let renamed = done.is_ok();
                self.emit(Action::SavedQueryDone(done)).await;
                if renamed {
                    self.list_saved_queries(project_id).await;
                }
            }
            crate::Effect::DeleteSavedQuery { project_id, id } => {
                let Some(storage) = &self.storage else {
                    return;
                };
                let done = storage
                    .delete_saved_query(project_id.clone(), id)
                    .await
                    .map(|()| "Deleted the saved query.".to_string())
                    .map_err(|error| error.to_string());
                self.emit(Action::SavedQueryDone(done)).await;
                self.list_saved_queries(project_id).await;
            }
            crate::Effect::LoadForeignKeys {
                session,
                generation,
                table,
            } => {
                let Some(active) = self.sessions.get(session) else {
                    self.emit(Action::ForeignKeysLoaded {
                        generation,
                        table,
                        result: Err("the session is closed".into()),
                    })
                    .await;
                    return;
                };
                let session = Arc::clone(&active.session);
                let action_tx = self.action_tx.clone();
                tokio::spawn(async move {
                    let result = match session.catalog() {
                        Some(catalog) => catalog
                            .foreign_keys(&table)
                            .await
                            .map_err(|error| error.to_string()),
                        None => Err("this connection has no catalog to ask".into()),
                    };
                    let _ = action_tx
                        .send(Action::ForeignKeysLoaded {
                            generation,
                            table,
                            result,
                        })
                        .await;
                });
            }
            crate::Effect::CancelCount { operation } => {
                let task = self
                    .counts
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&operation);
                if let Some(task) = task {
                    // The server stops counting first; dropping the task closes the
                    // connection it was counting on.
                    if let Some((session, query)) = task.running {
                        let _ = session.cancel(query).await;
                    }
                    if let Some(task) = task.task {
                        task.abort();
                    }
                }
            }
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
            crate::Effect::TouchRecentSqlFile { project_id, path } => {
                if let Some(storage) = &self.storage {
                    let _ = storage.touch_recent_sql_file(project_id, path);
                }
            }
            crate::Effect::AutosaveDocument {
                id,
                path,
                content,
                revision,
            } => self.autosave_document(id, path, content, revision).await,
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
                cursor,
                dialect,
                analyze,
                session,
                document,
                operation,
                generation: _,
            } => {
                let Some(active) = self.sessions.get(session) else {
                    self.emit(Action::ExplainFailed {
                        document,
                        operation,
                        message: "session is closed".into(),
                    })
                    .await;
                    return;
                };
                // Spawned, never awaited here: this arm runs on the loop that draws frames,
                // and an ANALYZE runs the whole statement -- the screen used to freeze
                // until it finished. The live slot is what Ctrl+F2 cancels.
                let session = Arc::clone(&active.session);
                let task = self.query.registry().register().id;
                *self.live.lock().await = Some(query_runner::LiveQuery {
                    task,
                    query: dexo_driver_api::QueryId(Uuid::new_v4()),
                    session: Arc::clone(&session),
                });
                let live = Arc::clone(&self.live);
                let registry = Arc::clone(self.query.registry());
                let action_tx = self.action_tx.clone();
                tokio::spawn(async move {
                    let request = explain_manager::ExplainRun {
                        cursor,
                        dialect,
                        analyze,
                        document,
                        operation,
                    };
                    explain_manager::run_live(session, &sql, request, action_tx).await;
                    let mut slot = live.lock().await;
                    if slot.as_ref().is_some_and(|live| live.task == task) {
                        *slot = None;
                    }
                    registry.finish(task);
                });
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
            crate::Effect::SettleApproval { id, approve } => {
                self.settle_approval(id, approve).await
            }
            crate::Effect::CheckApprovals => self.check_approvals().await,
            crate::Effect::SetMcpProfileEnabled { name, enabled } => {
                self.set_mcp_profile_enabled(name, enabled).await
            }
            crate::Effect::RevokeMcpGrants { profile } => self.revoke_mcp(profile).await,
            crate::Effect::RevokeAllMcpGrants => self.revoke_all_mcp().await,
            crate::Effect::WriteDiagnostics { path, bundle } => {
                diagnostic_manager::write(bundle, path, self.action_tx.clone()).await;
            }
            crate::Effect::RunTransfer(request) => self.dispatch_transfer(request).await,
            crate::Effect::SearchCompletionObjects {
                connection_id,
                database_name,
                document,
                revision,
                query,
                limit,
            } => {
                // Spawned, never awaited here: this arm runs on the loop that also draws
                // frames, and the search opens the catalog snapshot on disk.
                if let Some(storage) = self.storage.clone() {
                    let action_tx = self.action_tx.clone();
                    tokio::spawn(async move {
                        if let Ok(objects) = storage
                            .search_catalog_objects(connection_id, database_name, query, limit)
                            .await
                            && !objects.is_empty()
                        {
                            let _ = action_tx
                                .send(Action::CompletionObjectsLoaded {
                                    document,
                                    revision,
                                    objects,
                                })
                                .await;
                        }
                    });
                }
            }
            crate::Effect::LoadSnippets => {
                if let Some(storage) = &self.storage
                    && let Ok(snippets) = storage.list_snippets().await
                {
                    self.emit(Action::SnippetsLoaded(snippets)).await;
                }
            }
            crate::Effect::CheckpointRecovery(request) => self.checkpoint_recovery(request).await,
            crate::Effect::DiscardRecovery { document } => {
                if let Some(storage) = &self.storage {
                    let _ = storage.discard_recovery(document);
                }
            }
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
                    let driver_parent = parent.clone().filter(|id| {
                        id != &crate::screens::explorer::connection_id(&active.connection)
                    });
                    catalog_manager::load_children(
                        Arc::clone(&active.session),
                        parent,
                        driver_parent,
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
                ticket,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    data_manager::fetch_page(
                        Arc::clone(&active.session),
                        request,
                        generation,
                        ticket,
                        session,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
            crate::Effect::LoadTableColumns {
                target,
                session,
                generation,
                ticket,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    data_manager::fetch_table_columns(
                        Arc::clone(&active.session),
                        target,
                        generation,
                        ticket,
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
            // ponytail: the read runs on the loop; spawn it if a Wayland round trip
            // ever shows up as a stutter the way the connect did.
            crate::Effect::ReadClipboard => match clipboard::read_text() {
                Ok(text) if !text.is_empty() => self.emit(Action::Paste(text)).await,
                Ok(_) => {}
                Err(message) => self.emit(Action::ClipboardFailed { message }).await,
            },
            // Both paths, always: arboard can report success and still reach no other
            // program (XWayland, tmux, SSH), and the terminal cannot report at all.
            crate::Effect::CopyToClipboard { text } => {
                let terminal = clipboard::copy_via_terminal(&text);
                match clipboard::copy_text(text.clone()) {
                    Ok(()) => self.emit(Action::ClipboardWritten { text }).await,
                    Err(_) if terminal.is_ok() => {
                        self.emit(Action::ClipboardWritten { text }).await
                    }
                    Err(message) => self.emit(Action::ClipboardFailed { message }).await,
                }
            }
            crate::Effect::CaptureCatalogSnapshot {
                connection_id,
                database_name,
                session,
                generation,
                include_system,
                persist,
            } => {
                // Spawned: the walk visits every object in the database, and awaiting it
                // here froze the screen for as long as that took.
                if let Some(active) = self.sessions.get(session)
                    && let Ok(paths) = AppPaths::discover()
                {
                    tokio::spawn(catalog_manager::capture_snapshot(
                        Arc::clone(&active.session),
                        connection_id,
                        database_name,
                        include_system,
                        persist.then_some(paths.database),
                        generation,
                        self.action_tx.clone(),
                    ));
                }
            }
            crate::Effect::LoadCompletionCatalog {
                connection_id,
                database_name,
                generation,
            } => {
                let action_tx = self.action_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let objects = AppPaths::discover()
                        .ok()
                        .and_then(|paths| Database::open(&paths.database).ok())
                        .and_then(|db| {
                            dexo_storage::CatalogCache::new(db.connection())
                                .load_latest(&connection_id, &database_name)
                                .ok()
                        })
                        .unwrap_or_default();
                    if !objects.is_empty() {
                        let _ = action_tx.blocking_send(Action::CompletionCatalogLoaded {
                            generation,
                            objects,
                            complete: false,
                        });
                    }
                });
            }
            crate::Effect::LoadCompletionColumns {
                session,
                generation,
                target,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    let session = Arc::clone(&active.session);
                    let action_tx = self.action_tx.clone();
                    tokio::spawn(async move {
                        let Some(data) = session.data() else {
                            return;
                        };
                        let columns = data
                            .table_columns(&target)
                            .await
                            .map(|columns| columns.into_iter().map(|column| column.name).collect())
                            .unwrap_or_default();
                        let _ = action_tx
                            .send(Action::CompletionColumnsLoaded {
                                generation,
                                target,
                                columns,
                            })
                            .await;
                    });
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
            crate::Effect::CompleteOnboarding => {
                if let Ok(paths) = AppPaths::discover() {
                    let _ = crate::entrance::mark_complete(&paths.data_dir);
                }
            }
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

    async fn list_saved_queries(&self, project_id: String) {
        let Some(storage) = &self.storage else {
            return;
        };
        let listed = storage
            .list_saved_queries(project_id)
            .await
            .map_err(|error| error.to_string());
        self.emit(Action::SavedQueriesLoaded(listed)).await;
    }

    /// Counts on a connection dialled for it with the session's profile: a count on the
    /// session itself would wait behind its queries, and a cancel would stop theirs.
    async fn count_rows(
        &self,
        session: SessionId,
        operation: OperationId,
        sql: String,
        parameters: Vec<dexo_driver_api::DbValue>,
    ) {
        let fail = |message: String| Action::RowsCounted {
            operation,
            result: Err(message),
        };
        let Some(profile) = self.session_profiles.get(&session).cloned() else {
            return self.emit(fail("the session is closed".into())).await;
        };
        // What the session connected with, kept in memory -- a password command's
        // answer too -- before asking the keychain or the command again.
        let password = match self.secrets.memory.get(profile.secret_ref.as_str()) {
            Ok(Some(secret)) => Password::Ready(secret),
            _ => match Password::for_profile(&profile, &self.secrets) {
                Ok(password) => password,
                Err(_) => {
                    return self
                        .emit(fail("the password is not at hand; connect again".into()))
                        .await;
                }
            },
        };
        let factory = match self.drivers.get(&profile.driver) {
            Ok(factory) => factory,
            Err(error) => return self.emit(fail(error.to_string())).await,
        };
        let counts = Arc::clone(&self.counts);
        let memory = Arc::clone(&self.secrets.memory);
        let action_tx = self.action_tx.clone();
        counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(operation, CountTask::default());
        let slots = Arc::clone(&counts);
        let task = tokio::spawn(async move {
            let result = async {
                let (session, ..) = dial(factory, &profile, password, &memory, false).await?;
                let session: Arc<dyn dexo_driver_api::Session> = Arc::from(session);
                let mut request = dexo_driver_api::QueryRequest::read(sql, 1);
                request.parameters = parameters;
                {
                    let mut slots = slots
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let Some(slot) = slots.get_mut(&operation) else {
                        return Err("cancelled".to_string());
                    };
                    slot.running = Some((Arc::clone(&session), request.id));
                }
                let mut stream = session
                    .execute(request)
                    .await
                    .map_err(|error| error.to_string())?;
                let mut counted = None;
                while let Some(event) = futures_util::StreamExt::next(&mut stream).await {
                    if let dexo_driver_api::QueryEvent::Rows(batch) =
                        event.map_err(|error| error.to_string())?
                    {
                        counted = counted.or_else(|| {
                            batch
                                .rows
                                .first()
                                .and_then(|row| row.first())
                                .and_then(count_of)
                        });
                    }
                }
                counted.ok_or_else(|| "the count came back without a number".to_string())
            }
            .await;
            slots
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&operation);
            let _ = action_tx
                .send(Action::RowsCounted { operation, result })
                .await;
        });
        if let Some(slot) = counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&operation)
        {
            slot.task = Some(task.abort_handle());
        }
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
        let profile = match self.session_profiles.get(&session) {
            Some(profile) => profile.clone(),
            None => self.with_repo(|repo| {
                repo.list()
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .find(|profile| profile.name == name)
                    .ok_or_else(|| "connection profile not found".into())
            })?,
        };
        let secret = profile
            .password(&self.secrets)
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

    async fn create_connection(&mut self, input: NewConnection, password: String, connect: bool) {
        match self.save_profile(input, &password) {
            Ok((profile, SecretPersist::Stored)) => {
                self.emit(Action::ProfileSaved(profile.clone())).await;
                if connect {
                    self.connect_profile(profile, 0).await;
                }
            }
            // A temporary connection saved while the keychain is unavailable: saved all
            // the same, with the password kept for this session, as it already was.
            Ok((profile, SecretPersist::SessionOnly)) if !connect => {
                let _ = self
                    .secrets
                    .put_memory(profile.secret_ref.as_str(), &password);
                self.emit(Action::ProfileSaved(profile.clone())).await;
                self.emit(Action::Notice(format!(
                    "The keychain is unavailable: {} will ask for its password next time.",
                    profile.name
                )))
                .await;
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
        token: u64,
    ) {
        use crate::screens::secret_prompt::SecretChoiceKind;
        let key = profile.secret_ref.as_str();
        let result = match kind {
            SecretChoiceKind::Cancel => return,
            SecretChoiceKind::SessionOnly => self.secrets.put_memory(key, secret.expose()),
            SecretChoiceKind::SaveToKeychain => self.secrets.put_keychain(key, secret.expose()),
        };
        match result {
            Ok(()) => self.connect_profile(profile, token).await,
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
        if let Some((id, generation)) = self
            .sessions
            .find_by_connection(&profile.name)
            .map(|active| (active.id, active.generation))
        {
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
                driver: profile.driver,
            })
            .await;
            return;
        }
        // Everything up to the dial is local and fast; the dial itself is spawned, or a
        // host that never answers holds the whole event loop and the UI stops drawing.
        // A password command is part of the dial.
        let password = match Password::for_profile(&profile, &self.secrets) {
            Ok(password) => password,
            Err(action) => return self.emit(*action).await,
        };
        let from_command = matches!(password, Password::Command(_));
        let factory = match self.drivers.get(&profile.driver) {
            Ok(factory) => factory,
            Err(error) => {
                return self
                    .emit(Action::ConnectionFormError {
                        message: error.to_string(),
                    })
                    .await;
            }
        };
        let opening = Arc::clone(&self.opening);
        let action_tx = self.action_tx.clone();
        let memory = Arc::clone(&self.secrets.memory);
        tokio::spawn(async move {
            let action = match dial(factory, &profile, password, &memory, true).await {
                Ok((session, secret, effective)) => {
                    let printed = from_command.then_some(secret);
                    *opening.lock().await = Some((token, effective, Arc::from(session), printed));
                    Action::SessionOpened { token }
                }
                Err(message) => Action::ConnectionFormError { message },
            };
            let _ = action_tx.send(action).await;
        });
    }

    /// Takes the session a spawned connect parked and publishes it. A token that no
    /// longer matches means the user started another connect while this one dialled.
    async fn adopt_session(&mut self, token: u64) {
        let mut slot = self.opening.lock().await;
        if slot.as_ref().is_none_or(|(opened, ..)| *opened != token) {
            // A later connect has already parked over this one; taking the slot here
            // would drop its session on the floor.
            return;
        }
        let Some((_, profile, session, printed)) = slot.take() else {
            return;
        };
        drop(slot);
        if let Some(secret) = printed {
            let _ = self
                .secrets
                .put_memory(profile.secret_ref.as_str(), secret.expose_secret());
        }
        let unavailable = session
            .capabilities()
            .iter()
            .filter(|state| !state.available)
            .map(|state| {
                let reason = state.reason().unwrap_or("this driver cannot do that");
                (state.capability, reason.to_string())
            })
            .collect();
        let id = self.sessions.insert(profile.name.clone(), session);
        self.session_profiles.insert(id, profile.clone());
        // Before the connection is announced, so the palette is right from its first draw.
        self.emit(Action::SessionCapabilities {
            session: id,
            unavailable,
        })
        .await;
        let generation = self
            .sessions
            .get(id)
            .map(|active| active.generation)
            .unwrap_or(1);
        let read_only = dexo_app::ConnectionPolicy::resolve(&profile.environment, &profile.policy)
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
            driver: profile.driver,
        })
        .await;
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
        // A keychain that will not let go of the password is no reason to keep a
        // connection the user asked to delete; they are told what was left behind.
        let password_left = delete_secrets
            && matches!(
                self.secrets.delete(profile.secret_ref.as_str()),
                Err(SecretError::Unavailable) | Err(SecretError::Internal)
            );
        match self.with_repo(|repo| repo.delete(profile.id).map_err(|error| error.to_string())) {
            Ok(()) => {
                let name = profile.name.clone();
                self.emit(Action::ProfileDeleted { name }).await;
                if password_left {
                    self.emit(Action::ConnectionFormError {
                        message: format!(
                            "deleted {}, but its saved password could not be removed from the keychain",
                            profile.name
                        ),
                    })
                    .await;
                }
            }
            Err(message) => self.emit(Action::ConnectionFormError { message }).await,
        }
    }

    async fn test_input(&mut self, input: NewConnection, password: String) {
        match dexo_app::test_connection_input(input) {
            Ok(profile) => {
                if !profile.is_file()
                    && profile.password_command().is_none()
                    && let Err(error) = self
                        .secrets
                        .put_memory(profile.secret_ref.as_str(), &password)
                {
                    self.emit(Action::ConnectionFormError {
                        message: error.to_string(),
                    })
                    .await;
                    return;
                }
                self.spawn_test(profile);
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
        self.spawn_test(profile);
    }

    /// Dials `profile` once and reports whether it answered. Spawned like a connect: a
    /// host or a password command that never answers would otherwise stop the UI.
    fn spawn_test(&self, profile: ConnectionProfile) {
        let ready = match profile.password_command() {
            Some(command) => Ok(Password::Command(command.to_string())),
            None => profile
                .password(&self.secrets)
                .map_err(|error| error.to_string())
                .and_then(|secret| {
                    secret
                        .map(Password::Ready)
                        .ok_or_else(|| "secret is missing for this connection".to_string())
                }),
        }
        .and_then(|password| {
            self.drivers
                .get(&profile.driver)
                .map(|factory| (factory, password))
                .map_err(|error| error.to_string())
        });
        let action_tx = self.action_tx.clone();
        let memory = Arc::clone(&self.secrets.memory);
        tokio::spawn(async move {
            let answered = match ready {
                Ok((factory, password)) => dial(factory, &profile, password, &memory, false)
                    .await
                    .map(drop),
                Err(message) => Err(message),
            };
            let (ok, message) = match answered {
                Ok(()) => (true, "ok".into()),
                Err(message) => (false, message),
            };
            let _ = action_tx
                .send(Action::ConnectionTested {
                    name: profile.name,
                    ok,
                    message,
                })
                .await;
        });
    }

    async fn close_session(&mut self, session: SessionId) {
        self.sessions.remove(session);
        self.session_profiles.remove(&session);
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
                    path: request.path,
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
                let document = request.document.clone();
                let revision = request.revision;
                if let Some(storage) = &self.storage {
                    let _ = storage.save_document(request);
                }
                self.emit(Action::DocumentSaved { document, revision })
                    .await;
            }
            Err(document_io::DocumentIoError::ExternalConflict { path, .. }) => {
                self.emit(Action::DocumentConflict {
                    path: path.display().to_string(),
                })
                .await;
            }
            Err(error) => {
                self.emit(Action::DocumentSaveFailed {
                    document: request.document,
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
    async fn autosave_document(
        &mut self,
        id: String,
        path: std::path::PathBuf,
        content: String,
        revision: u64,
    ) {
        match document_io::save_sql_atomic(&path, &content).await {
            Ok(()) => self.emit(Action::DocumentAutosaved { id, revision }).await,
            Err(error) => {
                self.emit(Action::OperationFailed {
                    key: OperationKey::new(OperationId::new(), "", String::new(), 0),
                    message: error.to_string(),
                })
                .await;
            }
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
                    recent_sql_files: loaded.recent_sql_files,
                })
                .await;
            }
            Err(error) => self.fail_project(error).await,
        }
    }

    async fn close_project_sessions(&mut self) {
        for id in self.sessions.ids() {
            self.sessions.remove(id);
            self.session_profiles.remove(&id);
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
            Ok(preview) => self.emit(Action::ConfigPreviewed(preview)).await,
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
        // The grants list used to be fixture-only: nothing read the ledger, so the
        // screen showed a permanently empty section.
        let ledger = dexo_storage::SqliteGrantLedger::open(&paths.database).ok();
        let now = unix_seconds();
        let profiles = profiles
            .into_iter()
            .map(|profile| crate::screens::mcp_profiles::McpProfileSummary {
                scopes: profile.selectors.iter().map(ToString::to_string).collect(),
                tools: profile
                    .tool_rules
                    .iter()
                    .map(|rule| rule.tool.clone())
                    .collect(),
                grants: ledger
                    .as_ref()
                    .map(|ledger| grant_lines(ledger, &profile.name, now))
                    .unwrap_or_default(),
                name: profile.name,
                enabled: profile.enabled,
            })
            .collect();
        self.emit(Action::McpProfilesLoaded { profiles }).await;
    }

    async fn load_mcp_audit(&self) {
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(ledger) = dexo_storage::SqliteGrantLedger::open(&paths.database) else {
            return;
        };
        use dexo_app::mcp::GrantLedger;
        let now = unix_now();
        let events = ledger
            .recent_audits(50)
            .into_iter()
            .map(|event| {
                format!(
                    "{} {} {} {} {}",
                    event.profile, event.request, event.decision, event.target, event.status
                )
                .trim_end()
                .to_string()
            })
            .collect();
        let pending = ledger.pending_approvals(now);
        self.emit(Action::McpAuditLoaded {
            events,
            pending,
            now,
        })
        .await;
    }

    async fn settle_approval(&self, id: uuid::Uuid, approve: bool) {
        use dexo_app::mcp::{ApprovalDecision, GrantLedger};
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(ledger) = dexo_storage::SqliteGrantLedger::open(&paths.database) else {
            return;
        };
        let decision = if approve {
            ApprovalDecision::Approved
        } else {
            ApprovalDecision::Denied
        };
        match ledger.settle_approval(id, decision, unix_now()) {
            Ok(true) => {
                self.emit(Action::Notice(if approve {
                    "Approved: the agent's write runs now.".into()
                } else {
                    "Denied: the agent is told no.".into()
                }))
                .await
            }
            Ok(false) => {
                self.emit(Action::Notice(
                    "That request was already decided, or its time ran out.".into(),
                ))
                .await
            }
            Err(error) => self.emit(Action::Notice(error.to_string())).await,
        }
        self.load_mcp_audit().await;
    }

    async fn check_approvals(&self) {
        use dexo_app::mcp::GrantLedger;
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(ledger) = dexo_storage::SqliteGrantLedger::open(&paths.database) else {
            return;
        };
        let pending = ledger.pending_approvals(unix_now());
        self.emit(Action::ApprovalsWaiting(pending)).await;
    }

    async fn set_mcp_profile_enabled(&self, name: String, enabled: bool) {
        let Ok(paths) = AppPaths::discover() else {
            return;
        };
        let Ok(db) = Database::open(&paths.database) else {
            return;
        };
        let repo = dexo_storage::McpProfileRepository::new(db.connection());
        if let Ok(Some(mut profile)) = repo.get_by_name(&name) {
            profile.enabled = enabled;
            let _ = repo.save(&profile);
        }
        self.load_mcp_profiles().await;
    }

    async fn revoke_mcp(&self, profile: String) {
        let Ok(paths) = AppPaths::discover() else {
            self.emit(Action::McpRevokeFailed {
                message: "storage unavailable".into(),
            })
            .await;
            return;
        };
        let Ok(ledger) = dexo_storage::SqliteGrantLedger::open(&paths.database) else {
            self.emit(Action::McpRevokeFailed {
                message: "storage unavailable".into(),
            })
            .await;
            return;
        };
        use dexo_app::mcp::GrantLedger;
        match ledger.revoke_profile(&profile) {
            Ok(count) => {
                self.emit(Action::McpGrantsRevoked { count }).await;
                self.load_mcp_audit().await;
                self.load_mcp_profiles().await;
            }
            Err(error) => {
                self.emit(Action::McpRevokeFailed {
                    message: error.to_string(),
                })
                .await;
            }
        }
    }

    async fn revoke_all_mcp(&self) {
        let Ok(paths) = AppPaths::discover() else {
            self.emit(Action::McpRevokeFailed {
                message: "storage unavailable".into(),
            })
            .await;
            return;
        };
        let Ok(ledger) = dexo_storage::SqliteGrantLedger::open(&paths.database) else {
            self.emit(Action::McpRevokeFailed {
                message: "storage unavailable".into(),
            })
            .await;
            return;
        };
        match ledger.revoke_all() {
            Ok(count) => {
                self.emit(Action::McpGrantsRevoked { count }).await;
                self.load_mcp_audit().await;
                self.load_mcp_profiles().await;
            }
            Err(error) => {
                self.emit(Action::McpRevokeFailed {
                    message: error.to_string(),
                })
                .await;
            }
        }
    }

    pub fn action_tx(&self) -> &tokio::sync::mpsc::Sender<Action> {
        &self.action_tx
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// A profile's live grants, shaped for the profiles screen.
fn grant_lines(
    ledger: &dexo_storage::SqliteGrantLedger,
    profile: &str,
    now: i64,
) -> Vec<crate::screens::mcp_profiles::GrantLine> {
    use dexo_app::mcp::GrantLedger;
    ledger
        .active_grants(profile, now)
        .into_iter()
        .map(|grant| crate::screens::mcp_profiles::GrantLine {
            id: grant.id.to_string(),
            capability: grant.capability.as_str().into(),
            tools: grant.tools.join(","),
            expires_in_secs: grant.expires_at.saturating_sub(now),
            // What the grant narrowed the profile down to, which is the only part
            // of a grant the profile rows do not already show.
            diff: format!(
                "{} {}",
                grant.connection,
                grant
                    .selectors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        })
        .collect()
}

#[cfg(test)]
mod dial_tests {
    use std::sync::Arc;

    use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
    use dexo_driver_api::{ConnectRequest, ConnectionFactory, DriverError, DriverErrorCategory};
    use dexo_secrets::{MemorySecretStore, SecretStore};
    use secrecy::SecretString;

    use super::{Password, dial};

    struct Refuses;

    #[async_trait::async_trait]
    impl ConnectionFactory for Refuses {
        fn descriptor(&self) -> dexo_driver_api::DriverDescriptor {
            dexo_driver_api::DriverDescriptor {
                id: "postgres",
                display_name: "Refuses",
                default_port: 5432,
                options: dexo_driver_api::ConnectionOptions {
                    tls: false,
                    client_certificate: false,
                    ssh: false,
                    proxy: false,
                },
                file: false,
            }
        }

        async fn connect(
            &self,
            _: ConnectRequest,
        ) -> Result<Box<dyn dexo_driver_api::Session>, DriverError> {
            Err(DriverError::new(
                DriverErrorCategory::Authentication,
                "password authentication failed",
            ))
        }
    }

    /// Only a connect with a password held in memory forgets it and says to connect
    /// again; a password command's answer, or a test, is reported as it is.
    #[tokio::test]
    async fn only_a_connect_forgets_a_rejected_password() {
        let profile = ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(1)),
            None,
            "shop",
            "postgres",
            "local",
            serde_json::json!({"host": "h", "port": 5432, "username": "u", "database": "d"}),
            SecretRef::new("held".into()),
        );
        let memory = MemorySecretStore::default();
        memory.put("held", "wrong").unwrap();
        let factory: Arc<dyn ConnectionFactory> = Arc::new(Refuses);
        let ready = || Password::Ready(SecretString::from("wrong"));
        for (password, forget) in [
            (ready(), false),
            (Password::Command("printf wrong".into()), true),
        ] {
            let message = dial(Arc::clone(&factory), &profile, password, &memory, forget)
                .await
                .err()
                .unwrap();
            assert!(!message.contains("connect again"), "{message}");
            assert!(memory.get("held").unwrap().is_some());
        }
        let message = dial(factory, &profile, ready(), &memory, true)
            .await
            .err()
            .unwrap();
        assert!(message.contains("connect again"), "{message}");
        assert!(memory.get("held").unwrap().is_none());
    }
}
