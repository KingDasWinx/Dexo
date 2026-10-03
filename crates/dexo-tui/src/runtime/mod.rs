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

/// Puts the passwords of `from` under the references of its copy. The copy has secret
/// references of its own; without the password under them it was saved, and then could
/// not connect. False when one could not be kept anywhere.
fn copy_secrets(
    secrets: &SessionSecrets,
    from: &ConnectionProfile,
    to: &ConnectionProfile,
) -> bool {
    let mut pairs: Vec<(String, String)> = from
        .secret_refs
        .iter()
        .filter_map(|(purpose, old)| {
            Some((
                old.as_str().to_string(),
                to.secret_refs.get(purpose)?.as_str().to_string(),
            ))
        })
        .collect();
    for (old, new) in [
        (from.ssh_password_key(), to.ssh_password_key()),
        (from.ssh_passphrase_key(), to.ssh_passphrase_key()),
    ] {
        if let (Some(old), Some(new)) = (old, new) {
            pairs.push((old, new));
        }
    }
    let mut kept_all = true;
    for (old, new) in pairs {
        if let Ok(Some(secret)) = secrets.get(&old) {
            let value = secret.expose_secret();
            kept_all &= secrets.put_keychain(&new, value).is_ok()
                || secrets.put_memory(&new, value).is_ok();
        }
    }
    kept_all
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
/// Why a dial failed, and whether the server turned the password down.
#[derive(Debug)]
struct DialError {
    message: String,
    /// The server refused the login: a wrong password, or a user that does not exist.
    rejected: bool,
    /// The password this session held in memory was dropped because of it.
    forgot: bool,
}

impl std::fmt::Display for DialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        if self.forgot {
            f.write_str(" -- connect again to enter the password")?;
        }
        Ok(())
    }
}

impl From<String> for DialError {
    fn from(message: String) -> Self {
        Self {
            message,
            rejected: false,
            forgot: false,
        }
    }
}

/// What an SSH tunnel is given to authenticate with, besides the key file's path.
#[derive(Default)]
struct SshSecrets {
    password: Option<SecretString>,
    passphrase: Option<SecretString>,
}

impl From<Option<SecretString>> for SshSecrets {
    fn from(password: Option<SecretString>) -> Self {
        Self {
            password,
            passphrase: None,
        }
    }
}

impl WorkbenchRuntime {
    /// The SSH secrets this connection has kept. `Err` is the one that is missing, as the
    /// prompt that asks for it: the key's passphrase when the key file is encrypted, the
    /// password when there is no key file.
    fn ssh_secrets(
        &self,
        profile: &ConnectionProfile,
    ) -> Result<SshSecrets, crate::screens::secret_prompt::SecretPurpose> {
        use crate::screens::secret_prompt::SecretPurpose;
        let mut found = SshSecrets::default();
        if let Some(key) = profile.ssh_password_key() {
            match self.secrets.get(&key) {
                Ok(Some(secret)) => found.password = Some(secret),
                _ => return Err(SecretPurpose::SshPassword),
            }
        }
        if let (Some(path), Some(key)) = (profile.ssh_key_file(), profile.ssh_passphrase_key())
            && dexo_transport::key_needs_passphrase(&path)
        {
            match self.secrets.get(&key) {
                Ok(Some(secret)) => found.passphrase = Some(secret),
                _ => return Err(SecretPurpose::SshPassphrase),
            }
        }
        Ok(found)
    }
}

async fn dial(
    factory: Arc<dyn dexo_driver_api::ConnectionFactory>,
    profile: &ConnectionProfile,
    password: Password,
    ssh: impl Into<SshSecrets>,
    memory: &MemorySecretStore,
    forget: bool,
) -> Result<
    (
        Box<dyn dexo_driver_api::Session>,
        SecretString,
        ConnectionProfile,
    ),
    DialError,
> {
    let forget = forget && matches!(password, Password::Ready(_));
    let secret = password.resolve().await?;
    let mut secrets = dexo_driver_api::ConnectionSecrets::database_password(SecretString::from(
        secret.expose_secret().to_string(),
    ));
    let ssh = ssh.into();
    if let Some(password) = ssh.password {
        secrets.insert("ssh_password", password);
    }
    if let Some(passphrase) = ssh.passphrase {
        secrets.insert("ssh_passphrase", passphrase);
    }
    // A pre-connect command opens the way first; the session keeps it running, and the
    // profile it hands back dials that way, for the connections that follow.
    let opened =
        dexo_app::connect::open(factory.as_ref(), profile, secrets, Some(CONNECT_TIMEOUT)).await;
    match opened {
        Ok(opened) => Ok((opened.session, secret, opened.profile)),
        Err(
            dexo_app::connect::ConnectError::PreConnect(error)
            | dexo_app::connect::ConnectError::Setup(error),
        ) => Err(error.to_string().into()),
        Err(dexo_app::connect::ConnectError::Driver(error)) => {
            let rejected = error.category() == dexo_driver_api::DriverErrorCategory::Authentication;
            let message = map_driver_error(error).to_string();
            let key = profile.secret_ref.as_str();
            let forgot = forget && rejected && matches!(memory.get(key), Ok(Some(_)));
            if forgot {
                let _ = memory.delete(key);
            }
            Err(DialError {
                message,
                rejected,
                forgot,
            })
        }
        Err(dexo_app::connect::ConnectError::TimedOut(limit)) => Err(format!(
            "{} did not answer within {}s ({})",
            profile.name,
            limit.as_secs(),
            profile.target()
        )
        .into()),
    }
}

/// A secret typed at a prompt, on its way to the server to be tried.
#[derive(Clone)]
struct Typed {
    purpose: crate::screens::secret_prompt::SecretPurpose,
    buffer: crate::screens::secret_prompt::SecretBuffer,
    /// Keep it in the keychain once the server has taken it.
    keychain: bool,
}

/// A connection to open beside a session's own; see `WorkbenchRuntime::side_dial`.
struct SideDial {
    factory: Arc<dyn dexo_driver_api::ConnectionFactory>,
    profile: ConnectionProfile,
    password: Password,
    ssh: SshSecrets,
    memory: Arc<MemorySecretStore>,
}

impl SideDial {
    async fn open(self) -> Result<Box<dyn dexo_driver_api::Session>, String> {
        dial(
            self.factory,
            &self.profile,
            self.password,
            self.ssh,
            &self.memory,
            false,
        )
        .await
        .map(|(session, ..)| session)
        .map_err(|error| error.to_string())
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
    /// Secrets typed at a prompt that the keychain gets once a connect has used them: the
    /// connect they belong to, the keychain key, and the secret.
    to_keychain: Vec<(u64, String, SecretString)>,
    /// Reads from a database for the screen -- a catalog node, a table's page, an
    /// object's details -- run one after another, in the order asked, off the loop that
    /// draws the screen and reads the keys. Awaited there, each froze the screen for its
    /// round trips: seconds on a server far away.
    reads: tokio::sync::mpsc::UnboundedSender<futures_util::future::BoxFuture<'static, ()>>,
}

/// The queue `WorkbenchRuntime::reads` sends to: each read, run to its end before the
/// next. ponytail: one lane for every session, so a slow server's read holds up another
/// server's behind it, as the loop did; a lane per session if that shows.
fn read_lane() -> tokio::sync::mpsc::UnboundedSender<futures_util::future::BoxFuture<'static, ()>> {
    let (lane, mut queue) =
        tokio::sync::mpsc::unbounded_channel::<futures_util::future::BoxFuture<'static, ()>>();
    tokio::spawn(async move {
        while let Some(read) = queue.recv().await {
            read.await;
        }
    });
    lane
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
            to_keychain: Vec::new(),
            reads: read_lane(),
        }
    }

    /// Queues `read` behind the reads asked for before it; see `reads`.
    fn queue_read(&self, read: impl std::future::Future<Output = ()> + Send + 'static) {
        let _ = self.reads.send(Box::pin(read));
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
                purpose,
                profile,
                secret,
                token,
            } => {
                self.submit_secret(kind, purpose, profile, secret, token)
                    .await
            }
            crate::Effect::DuplicateProfile { id, taken } => {
                self.duplicate_profile(id, taken).await
            }
            crate::Effect::TestConnection { input, password } => {
                self.test_input(input, password).await
            }
            crate::Effect::TestSavedProfile { profile } => self.test_saved(profile).await,
            crate::Effect::SaveProfile { profile, password } => {
                self.save_existing(profile, password).await
            }
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
                on_session,
            } => {
                self.count_rows(session, operation, sql, parameters, on_session)
                    .await
            }
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
                        })
                        .map(|()| Some(note).filter(|note| !note.is_empty()));
                    let _ = action_tx.blocking_send(Action::NoteSaved { object, saved });
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
                    self.queue_read(schema_manager::preview_live(
                        Arc::clone(&active.session),
                        session.0.to_string(),
                        change,
                        self.action_tx.clone(),
                    ));
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
            crate::Effect::LoadSchemaSources => {
                tokio::spawn(schema_manager::list_sources(self.action_tx.clone()));
            }
            crate::Effect::LoadSchemaDiff {
                left,
                right,
                render_session,
                generation: _,
            } => {
                let session_of = |side: &crate::action::DiffSide| match side {
                    crate::action::DiffSide::Live { session, .. } => self
                        .sessions
                        .get(*session)
                        .map(|active| Arc::clone(&active.session)),
                    _ => None,
                };
                let sides = (session_of(&left), session_of(&right));
                let render = render_session
                    .and_then(|id| self.sessions.get(id))
                    .map(|active| Arc::clone(&active.session));
                // Spawned: both catalogs are read whole, which takes as long as they are big.
                tokio::spawn(schema_manager::compare(
                    (left, sides.0),
                    (right, sides.1),
                    render,
                    self.action_tx.clone(),
                ));
            }
            crate::Effect::LoadSecurity {
                session,
                generation: _,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    self.queue_read(Self::load_security_session(
                        Arc::clone(&active.session),
                        self.action_tx.clone(),
                    ));
                }
            }
            crate::Effect::RunExplain {
                sql,
                cursor,
                dialect,
                analyze,
                indexes,
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
                        indexes,
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
                self.admin_on_side_connection(session, None).await
            }
            crate::Effect::AdminTerminate { session, target } => {
                let action = dexo_driver_api::AdminAction::TerminateSession { session_id: target };
                self.admin_on_side_connection(session, Some(action)).await
            }
            crate::Effect::LoadAdminView { session, view } => {
                let dial = match self.side_dial(session) {
                    Ok(dial) => dial,
                    Err(message) => {
                        return self
                            .emit(Action::AdminViewLoaded {
                                session,
                                view,
                                result: Err(message),
                            })
                            .await;
                    }
                };
                let action_tx = self.action_tx.clone();
                tokio::spawn(async move {
                    let action = match dial.open().await {
                        Ok(side) => admin_manager::load_view(Arc::from(side), session, view).await,
                        Err(message) => Action::AdminViewLoaded {
                            session,
                            view,
                            result: Err(message),
                        },
                    };
                    let _ = action_tx.send(action).await;
                });
            }
            crate::Effect::AdminCancel { session, target } => {
                let action = dexo_driver_api::AdminAction::CancelQuery { session_id: target };
                self.admin_on_side_connection(session, Some(action)).await
            }
            crate::Effect::LoadMcpProfiles => self.load_mcp_profiles().await,
            crate::Effect::LoadMcpClients => {
                let action_tx = self.action_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = action_tx.blocking_send(mcp_clients());
                });
            }
            crate::Effect::SetUpMcpClient {
                client,
                profile,
                skill,
            } => {
                self.off_the_loop(move || {
                    let result = set_up_mcp_client(client, profile, skill);
                    [Action::McpClientSetUp { result }, mcp_clients()]
                        .into_iter()
                        .chain(mcp_profiles_loaded())
                        .collect()
                });
            }
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
            crate::Effect::SaveMcpProfileAccess {
                name,
                connections,
                reads,
            } => {
                self.off_the_loop(move || {
                    let message = save_mcp_profile_access(&name, connections, reads)
                        .unwrap_or_else(|error| error);
                    std::iter::once(Action::McpProfileSaved { message })
                        .chain(mcp_profiles_loaded())
                        .collect()
                });
            }
            crate::Effect::RevokeMcpGrants { profile } => self.revoke_mcp(profile).await,
            crate::Effect::DeleteMcpProfile { name } => self.delete_mcp_profile(name).await,
            crate::Effect::RevokeAllMcpGrants => self.revoke_all_mcp().await,
            crate::Effect::CreateMcpGrant { profile, request } => {
                let action_tx = self.action_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let action = match create_mcp_grant(&profile, &request) {
                        Ok(message) => Action::McpGrantCreated { message },
                        Err(message) => Action::McpGrantFailed { message },
                    };
                    let _ = action_tx.blocking_send(action);
                });
            }
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
                if let Some(storage) = self.storage.clone() {
                    let action_tx = self.action_tx.clone();
                    tokio::spawn(async move {
                        if let Ok(snippets) = storage.list_snippets().await {
                            let _ = action_tx.send(Action::SnippetsLoaded(snippets)).await;
                        }
                    });
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
            crate::Effect::DeleteHistory { ids } => {
                if let Some(storage) = &self.storage {
                    let _ = storage.delete_history(ids);
                }
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
                    self.queue_read(catalog_manager::load_children(
                        Arc::clone(&active.session),
                        parent,
                        driver_parent,
                        operation,
                        session,
                        generation,
                        replace_roots,
                        include_system,
                        self.action_tx.clone(),
                    ));
                }
            }
            crate::Effect::LoadObjectInspector {
                id,
                session,
                generation,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    self.queue_read(catalog_manager::load_inspector(
                        Arc::clone(&active.session),
                        id,
                        generation,
                        session,
                        self.action_tx.clone(),
                    ));
                }
            }
            crate::Effect::LoadTableData {
                request,
                session,
                generation,
                ticket,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    self.queue_read(data_manager::fetch_page(
                        Arc::clone(&active.session),
                        request,
                        generation,
                        ticket,
                        session,
                        self.action_tx.clone(),
                    ));
                }
            }
            crate::Effect::LoadTableColumns {
                target,
                session,
                generation,
                ticket,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    self.queue_read(data_manager::fetch_table_columns(
                        Arc::clone(&active.session),
                        target,
                        generation,
                        ticket,
                        self.action_tx.clone(),
                    ));
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
                    self.queue_read(data_manager::fetch_value(
                        Arc::clone(&active.session),
                        value,
                        offset,
                        limit,
                        generation,
                        self.action_tx.clone(),
                    ));
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
            // Off the loop: on X11 arboard waits up to four seconds for whoever holds the
            // clipboard to answer, and Ctrl+V froze the editor that long.
            crate::Effect::ReadClipboard => self.off_the_loop(|| match clipboard::read_text() {
                Ok(text) if !text.is_empty() => vec![Action::Paste(text)],
                Ok(_) => Vec::new(),
                Err(_) => vec![Action::ClipboardUnreadable],
            }),
            // Both paths, always: arboard can report success and still reach no other
            // program (XWayland, tmux, SSH), and the terminal cannot report at all. The
            // terminal's is written here, beside the frames; arboard's off the loop, as
            // it hands the text over to a clipboard manager.
            crate::Effect::CopyToClipboard { text } => {
                let terminal = clipboard::copy_via_terminal(&text).is_ok();
                self.off_the_loop(move || {
                    vec![match clipboard::copy_text(text.clone()) {
                        Ok(()) => Action::ClipboardWritten { text },
                        Err(_) if terminal => Action::ClipboardWritten { text },
                        Err(message) => Action::ClipboardFailed { message },
                    }]
                });
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
        // Spawned, never awaited here: an export, an import or a restore takes as long as
        // the data does, and this arm runs on the loop that draws the frames and reads
        // the keys, Esc among them.
        let manager = self.transfer.clone();
        tokio::spawn(async move {
            let _ = manager.run_with(request, Some(&access)).await;
        });
    }

    async fn list_saved_queries(&self, project_id: String) {
        let Some(storage) = self.storage.clone() else {
            return;
        };
        let action_tx = self.action_tx.clone();
        tokio::spawn(async move {
            let listed = storage
                .list_saved_queries(project_id)
                .await
                .map_err(|error| error.to_string());
            let _ = action_tx.send(Action::SavedQueriesLoaded(listed)).await;
        });
    }

    /// Counts on a connection dialled for it with the session's profile: a count on the
    /// session itself would wait behind its queries, and a cancel would stop theirs.
    async fn count_rows(
        &self,
        session: SessionId,
        operation: OperationId,
        sql: String,
        parameters: Vec<dexo_driver_api::DbValue>,
        on_session: bool,
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
        // A password command is asked again: its answer may have rotated since.
        let password = match self.secrets.memory.get(profile.secret_ref.as_str()) {
            Ok(Some(secret)) if profile.password_command().is_none() => Password::Ready(secret),
            _ => match Password::for_profile(&profile, &self.secrets) {
                Ok(password) => password,
                Err(_) => {
                    return self
                        .emit(fail("the password is not at hand; connect again".into()))
                        .await;
                }
            },
        };
        // The tunnel the session went through needs the password it was opened with.
        let ssh = self.ssh_secrets(&profile).unwrap_or_default();
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
        // A result's count reads what its session reads: its search path, temporary
        // tables, open transaction.
        let live = on_session
            .then(|| {
                self.sessions
                    .get(session)
                    .map(|active| Arc::clone(&active.session))
            })
            .flatten();
        let task = tokio::spawn(async move {
            let result = async {
                let session: Arc<dyn dexo_driver_api::Session> = match live {
                    Some(session) => session,
                    None => {
                        let (session, ..) = dial(factory, &profile, password, ssh, &memory, false)
                            .await
                            .map_err(|error| error.to_string())?;
                        Arc::from(session)
                    }
                };
                let mut request = dexo_driver_api::QueryRequest::read(sql, 1);
                request.parameters = parameters;
                // Its WHERE is text from the bars: run where it cannot write.
                request.read_only = true;
                // No limit of its own: an exact count of a big table takes what it takes,
                // and `t` stops it.
                request.timeout = Duration::ZERO;
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

    /// What a task needs to open a connection of its own to what `session` is on. The
    /// session's own connection can be busy -- a statement waiting on a lock -- and
    /// whatever it is asked then waits behind that statement.
    fn side_dial(&self, session: SessionId) -> Result<SideDial, String> {
        let profile = self
            .session_profiles
            .get(&session)
            .cloned()
            .ok_or_else(|| "the session is closed".to_string())?;
        // A password command is asked again: its answer may have rotated since.
        let password = match self.secrets.memory.get(profile.secret_ref.as_str()) {
            Ok(Some(secret)) if profile.password_command().is_none() => Password::Ready(secret),
            _ => Password::for_profile(&profile, &self.secrets)
                .map_err(|_| "the password is not at hand; connect again".to_string())?,
        };
        let factory = self
            .drivers
            .get(&profile.driver)
            .map_err(|error| error.to_string())?;
        // A tunnel's secrets are the ones the session was opened with.
        let ssh = self
            .ssh_secrets(&profile)
            .map_err(|_| "the SSH secret is not at hand; connect again".to_string())?;
        Ok(SideDial {
            factory,
            profile,
            password,
            ssh,
            memory: Arc::clone(&self.secrets.memory),
        })
    }

    /// Lists the server's sessions, or ends `terminate`'s, on a connection dialled for
    /// it and spawned: the list is the tool for a stuck session, so it cannot wait on
    /// that session, and the loop that draws the screen cannot wait on either. A
    /// connection of its own also reads outside the session's open transaction, whose
    /// snapshot of the server's activity does not move until it ends.
    async fn admin_on_side_connection(
        &self,
        session: SessionId,
        act: Option<dexo_driver_api::AdminAction>,
    ) {
        let failed = |message: String, act: &Option<dexo_driver_api::AdminAction>| match act {
            Some(act) => admin_manager::acted(act, Err(message)),
            None => Action::AdminFailed { message },
        };
        let dial = match self.side_dial(session) {
            Ok(dial) => dial,
            Err(message) => return self.emit(failed(message, &act)).await,
        };
        let action_tx = self.action_tx.clone();
        tokio::spawn(async move {
            match dial.open().await {
                Ok(side) => {
                    let side: Arc<dyn dexo_driver_api::Session> = Arc::from(side);
                    match act {
                        Some(act) => admin_manager::act_live(side, act, action_tx).await,
                        None => admin_manager::load_live(side, action_tx).await,
                    }
                }
                Err(message) => {
                    let _ = action_tx.send(failed(message, &act)).await;
                }
            }
        });
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
        purpose: crate::screens::secret_prompt::SecretPurpose,
        profile: ConnectionProfile,
        secret: crate::screens::secret_prompt::SecretBuffer,
        token: u64,
    ) {
        use crate::screens::secret_prompt::{SecretChoiceKind, SecretPurpose};
        let key = match purpose {
            SecretPurpose::SshPassword => profile.ssh_password_key(),
            SecretPurpose::SshPassphrase => profile.ssh_passphrase_key(),
            _ => Some(profile.secret_ref.as_str().to_string()),
        };
        let Some(key) = key else { return };
        let keychain = match kind {
            SecretChoiceKind::Cancel => return,
            SecretChoiceKind::SessionOnly => false,
            SecretChoiceKind::SaveToKeychain => true,
        };
        // Held in memory while the server is asked: a password it turns down is not kept,
        // where it used to be written to the keychain first.
        if let Err(error) = self.secrets.put_memory(&key, secret.expose()) {
            return self
                .emit(Action::ConnectionFormError {
                    message: error.to_string(),
                })
                .await;
        }
        if keychain {
            // A password tried before and turned down is not the one to keep.
            self.to_keychain
                .retain(|(waiting, kept, _)| !(*waiting == token && *kept == key));
            self.to_keychain
                .push((token, key, SecretString::from(secret.expose())));
        }
        let typed = Typed {
            purpose,
            buffer: secret,
            keychain,
        };
        self.connect_profile_with(profile, token, Some(typed)).await;
    }

    async fn connect_profile(&mut self, profile: ConnectionProfile, token: u64) {
        self.connect_profile_with(profile, token, None).await;
    }

    async fn connect_profile_with(
        &mut self,
        profile: ConnectionProfile,
        token: u64,
        typed: Option<Typed>,
    ) {
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
        // A tunnel through SSH without a key file needs the SSH password too: asked for
        // here, like the database's, rather than refused with the name of its key.
        let ssh_password = match self.ssh_secrets(&profile) {
            Ok(secrets) => secrets,
            Err(purpose) => {
                return self
                    .emit(Action::SecretRequired {
                        purpose,
                        profile,
                        buffer: crate::screens::secret_prompt::SecretBuffer::new(String::new()),
                    })
                    .await;
            }
        };
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
            let dialled = dial(factory, &profile, password, ssh_password, &memory, true).await;
            let action = match dialled {
                Ok((session, secret, effective)) => {
                    let printed = from_command.then_some(secret);
                    *opening.lock().await = Some((token, effective, Arc::from(session), printed));
                    Action::SessionOpened { token }
                }
                // A password the server turned down comes back to the prompt it was typed
                // at, with what the server said, to be typed again.
                Err(error) if error.rejected && typed.is_some() => {
                    let typed = typed.unwrap_or_else(|| unreachable!("checked above"));
                    Action::SecretRejected {
                        purpose: typed.purpose,
                        profile,
                        buffer: typed.buffer,
                        keychain: typed.keychain,
                        message: error.message,
                    }
                }
                Err(error) => {
                    // The SSH password that was just typed did not get a session either.
                    use crate::screens::secret_prompt::SecretPurpose;
                    if let Some(typed) = &typed {
                        let key = match typed.purpose {
                            SecretPurpose::SshPassword => profile.ssh_password_key(),
                            SecretPurpose::SshPassphrase => profile.ssh_passphrase_key(),
                            _ => None,
                        };
                        if let Some(key) = key {
                            let _ = memory.delete(&key);
                        }
                    }
                    Action::ConnectionFormError {
                        message: format!("{}: {error}", profile.name),
                    }
                }
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
        // The server took what was typed: now it may go to the keychain.
        let kept: Vec<(u64, String, SecretString)> = self.to_keychain.drain(..).collect();
        for (waiting, key, secret) in kept {
            if waiting != token {
                continue;
            }
            match self.secrets.put_keychain(&key, secret.expose_secret()) {
                Ok(()) => {
                    let _ = self.secrets.memory.delete(&key);
                }
                Err(_) => {
                    self.emit(Action::Notice(format!(
                        "The keychain is unavailable: {} will ask for its password next time.",
                        profile.name
                    )))
                    .await;
                }
            }
        }
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
        let server_id = self
            .sessions
            .get(id)
            .and_then(|active| active.session.server_session_id());
        if let Some(server_id) = server_id {
            self.emit(Action::SessionServerId {
                session: id,
                server_id,
            })
            .await;
        }
    }

    async fn duplicate_profile(&mut self, id: dexo_app::ConnectionId, taken: Vec<String>) {
        match self.with_repo(|repo| {
            let original = repo.get(id).map_err(|error| error.to_string())?;
            let copy = repo
                .duplicate(id, &taken)
                .map_err(|error| error.to_string())?;
            Ok((original, copy))
        }) {
            Ok((original, copy)) => {
                let kept_all = match &original {
                    Some(original) => copy_secrets(&self.secrets, original, &copy),
                    None => true,
                };
                self.emit(Action::ProfileSaved(copy.clone())).await;
                if !kept_all {
                    self.emit(Action::Notice(format!(
                        "{} has no password: the original's could not be copied.",
                        copy.name
                    )))
                    .await;
                }
            }
            Err(message) => self.emit(Action::ConnectionFormError { message }).await,
        }
    }

    async fn save_existing(&mut self, profile: ConnectionProfile, password: String) {
        match self.with_repo(|repo| repo.update(&profile).map_err(|error| error.to_string())) {
            Ok(()) => {
                // The edit form saved the profile and dropped the typed password with
                // it, which is how a changed password used to say "saved" and keep the
                // old one. Empty means leave the saved one alone.
                if !password.is_empty()
                    && !profile.is_file()
                    && profile.password_command().is_none()
                {
                    self.store_password(&profile, &password).await;
                }
                self.emit(Action::ProfileSaved(profile)).await
            }
            Err(message) => self.emit(Action::ConnectionFormError { message }).await,
        }
    }

    /// Keeps `password` as the connection's, in the keychain; for this session only when
    /// the keychain is unavailable, which the user is told.
    async fn store_password(&mut self, profile: &ConnectionProfile, password: &str) {
        let key = profile.secret_ref.as_str();
        // A password kept for this session would otherwise be read before the new one.
        let _ = self.secrets.memory.delete(key);
        match self.secrets.put_keychain(key, password) {
            Ok(()) => {}
            Err(SecretError::Unavailable) => {
                let _ = self.secrets.put_memory(key, password);
                self.emit(Action::Notice(format!(
                    "The keychain is unavailable: {} will ask for its password next time.",
                    profile.name
                )))
                .await;
            }
            Err(error) => {
                self.emit(Action::ConnectionFormError {
                    message: format!("saved {}, but not its new password: {error}", profile.name),
                })
                .await;
            }
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
        if delete_secrets {
            for key in [profile.ssh_password_key(), profile.ssh_passphrase_key()]
                .into_iter()
                .flatten()
            {
                let _ = self.secrets.delete(&key);
            }
        }
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
            // A test cannot ask: the SSH secrets are the ones kept, or none.
            let ssh = self.ssh_secrets(&profile).map_err(|_| {
                "the SSH tunnel needs a password or a key passphrase: connect the connection \
                 once, and it asks for it"
                    .to_string()
            });
            ssh.map(|ssh| (password, ssh))
        })
        .and_then(|(password, ssh)| {
            self.drivers
                .get(&profile.driver)
                .map(|factory| (factory, password, ssh))
                .map_err(|error| error.to_string())
        });
        let action_tx = self.action_tx.clone();
        let memory = Arc::clone(&self.secrets.memory);
        tokio::spawn(async move {
            let answered = match ready {
                Ok((factory, password, ssh)) => {
                    dial(factory, &profile, password, ssh, &memory, false)
                        .await
                        .map(drop)
                        .map_err(|error| error.to_string())
                }
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
            // Said as what the user can do about it: `session is closed` named neither
            // the cause nor the way out.
            let message = if request.key.session.is_empty() {
                "This document has no connection. Pick one in the explorer (Alt+1, then Enter) and run it again."
            } else {
                "The connection this document runs on is closed. Connect it from the explorer and run it again."
            };
            self.emit(Action::OperationFailed {
                key: request.key,
                message: message.into(),
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
        self.tx_result(session, result, "Transaction started.".into())
            .await;
    }

    async fn commit(&mut self, session: SessionId) {
        let result = self.sessions.commit(session).await;
        self.tx_result(session, result, "Transaction committed.".into())
            .await;
    }

    async fn rollback(&mut self, session: SessionId) {
        let result = self.sessions.rollback(session).await;
        // A server that ended the rollback with a warning says what it did not undo.
        let notice = self
            .sessions
            .get(session)
            .and_then(|active| active.session.transactions()?.take_notice());
        let done = match notice {
            Some(notice) => format!("Transaction rolled back. {notice}"),
            None => "Transaction rolled back.".into(),
        };
        self.tx_result(session, result, done).await;
    }

    async fn savepoint(&mut self, session: SessionId, name: String) {
        let result = self.sessions.savepoint(session, &name).await;
        self.tx_result(session, result, format!("Savepoint {name} created."))
            .await;
    }

    async fn rollback_to(&mut self, session: SessionId, name: String) {
        let result = self.sessions.rollback_to(session, &name).await;
        self.tx_result(session, result, format!("Rolled back to savepoint {name}."))
            .await;
    }

    async fn release_savepoint(&mut self, session: SessionId, name: String) {
        let result = self.sessions.release_savepoint(session, &name).await;
        self.tx_result(session, result, format!("Savepoint {name} released."))
            .await;
    }

    /// What a transaction command did, as the state it left and a line that says so; or
    /// why it could not.
    async fn tx_result(
        &self,
        session: SessionId,
        result: Result<dexo_driver_api::TransactionState, String>,
        done: String,
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
                self.emit(Action::Notice(done)).await;
            }
            Err(message) => {
                self.emit(Action::TransactionFailed { message }).await;
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
                self.emit(Action::DocumentLoadFailed {
                    document: request.document,
                    message: document_io::load_failure(&request.path, &error),
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
                    documents: loaded.documents,
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
            Ok(()) => self.emit(Action::ConfigExported).await,
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
                    commands: report.commands,
                })
                .await;
                // What was imported is in the database; the sidebar and the project list
                // read it from there, and showed the old ones until a restart.
                if let Ok(projects) = storage.list_projects().await {
                    self.emit(Action::ProjectsLoaded(projects)).await;
                }
                if let Ok(profiles) = self.with_repo(|repo| repo.list().map_err(|e| e.to_string()))
                {
                    self.emit(Action::ProfilesLoaded(profiles)).await;
                }
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
            let _ = storage.persist_history(request);
        }
    }

    /// Spawned, as every read the storage worker answers: see `check_approvals`.
    async fn load_history(&mut self, connection_id: Option<String>) {
        let Some(storage) = self.storage.clone() else {
            return;
        };
        let action_tx = self.action_tx.clone();
        tokio::spawn(async move {
            let action = match storage.list_history(connection_id).await {
                Ok(entries) => Action::HistoryLoaded(entries),
                Err(error) => Action::OperationFailed {
                    key: OperationKey::new(OperationId::new(), "", "", 0),
                    message: error.to_string(),
                },
            };
            let _ = action_tx.send(action).await;
        });
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

    /// Runs `work` -- reads and writes of Dexo's database -- on a thread of its own and
    /// sends what it answers. SQLite waits out a lock another process holds, up to five
    /// seconds, and the loop that draws the screen waited with it: Agents read its audit
    /// every second, and the favorites were read after every catalog node.
    fn off_the_loop(&self, work: impl FnOnce() -> Vec<Action> + Send + 'static) {
        let action_tx = self.action_tx.clone();
        tokio::task::spawn_blocking(move || {
            for action in work() {
                let _ = action_tx.blocking_send(action);
            }
        });
    }

    async fn load_offline_catalog(
        &self,
        connection_id: String,
        database_name: String,
        generation: u64,
    ) {
        self.off_the_loop(move || {
            let Ok(paths) = AppPaths::discover() else {
                return Vec::new();
            };
            let Ok(db) = Database::open(&paths.database) else {
                return Vec::new();
            };
            let cache = dexo_storage::CatalogCache::new(db.connection());
            let created_at = cache
                .latest_metadata(&connection_id, &database_name)
                .ok()
                .flatten()
                .map(|meta| meta.created_at);
            let Ok(objects) = cache.load_latest(&connection_id, &database_name) else {
                return Vec::new();
            };
            vec![Action::OfflineCatalogLoaded {
                generation,
                list: dexo_driver_api::CatalogList {
                    objects,
                    restrictions: vec![],
                },
                created_at,
            }]
        });
    }

    async fn load_object_usage(&self, project_id: String, connection_id: String) {
        self.off_the_loop(move || {
            let Ok(paths) = AppPaths::discover() else {
                return Vec::new();
            };
            let Ok(db) = Database::open(&paths.database) else {
                return Vec::new();
            };
            let Ok(rows) = dexo_storage::ObjectUsageRepository::new(db.connection())
                .list_for_connection(&project_id, &connection_id)
            else {
                return Vec::new();
            };
            let ids = rows
                .into_iter()
                .filter(|row| row.favorite)
                .map(|row| row.object_id)
                .collect();
            vec![Action::ApplyFavorites { ids }]
        });
    }

    fn persist_favorite(
        &self,
        project_id: String,
        connection_id: String,
        object_id: String,
        favorite: bool,
    ) {
        // ponytail: second rusqlite handle; fold into StorageCommand if catalog writes contend.
        self.off_the_loop(move || {
            if let Ok(paths) = AppPaths::discover()
                && let Ok(db) = Database::open(&paths.database)
            {
                let _ = dexo_storage::ObjectUsageRepository::new(db.connection()).set_favorite(
                    &project_id,
                    &connection_id,
                    &object_id,
                    favorite,
                );
            }
            Vec::new()
        });
    }

    async fn load_mcp_profiles(&self) {
        self.off_the_loop(|| mcp_profiles_loaded().into_iter().collect());
    }

    async fn load_mcp_audit(&self) {
        self.off_the_loop(|| mcp_audit_loaded().into_iter().collect());
    }

    async fn settle_approval(&self, id: uuid::Uuid, approve: bool) {
        self.off_the_loop(move || {
            use dexo_app::mcp::approval::{Answered, answer};
            let Ok(paths) = AppPaths::discover() else {
                return Vec::new();
            };
            let Ok(ledger) = dexo_storage::SqliteGrantLedger::open(&paths.database) else {
                return Vec::new();
            };
            let message = match answer(&ledger, id, approve, unix_now()) {
                Ok(Answered::Taken) if approve => "Approved: the agent's write runs now.".into(),
                Ok(Answered::Taken) => "Denied: the agent is told no.".into(),
                Ok(Answered::AlreadyDecided) => {
                    "That request was already decided, or its grant was revoked.".into()
                }
                Ok(Answered::TimedOut) => "That request's time ran out; nothing runs.".into(),
                Ok(Answered::NobodyWaiting) => {
                    "The agent is no longer waiting for this write; nothing runs.".into()
                }
                Err(error) => error.to_string(),
            };
            std::iter::once(Action::Notice(message))
                .chain(mcp_audit_loaded())
                .collect()
        });
    }

    /// Asked every two seconds, on the storage worker's open connection: it opened a
    /// database and wrote to it each time, for people who never use MCP too. Spawned,
    /// never awaited here: the worker answers after whatever it is doing, and a write of
    /// its waits for a lock another process -- the MCP server -- holds on the database.
    /// Every two seconds the screen froze with it.
    async fn check_approvals(&self) {
        let Some(storage) = self.storage.clone() else {
            return;
        };
        let action_tx = self.action_tx.clone();
        tokio::spawn(async move {
            if let Ok(pending) = storage.waiting_approvals(unix_now()).await {
                let _ = action_tx.send(Action::ApprovalsWaiting(pending)).await;
            }
        });
    }

    async fn set_mcp_profile_enabled(&self, name: String, enabled: bool) {
        self.off_the_loop(move || {
            if let Ok(paths) = AppPaths::discover()
                && let Ok(db) = Database::open(&paths.database)
            {
                let repo = dexo_storage::McpProfileRepository::new(db.connection());
                if let Ok(Some(mut profile)) = repo.get_by_name(&name) {
                    profile.enabled = enabled;
                    let _ = repo.save(&profile);
                }
            }
            mcp_profiles_loaded().into_iter().collect()
        });
    }

    async fn revoke_mcp(&self, profile: String) {
        self.off_the_loop(move || {
            use dexo_app::mcp::GrantLedger;
            let unavailable = || {
                vec![Action::McpRevokeFailed {
                    message: "storage unavailable".into(),
                }]
            };
            let Ok(paths) = AppPaths::discover() else {
                return unavailable();
            };
            let Ok(ledger) = dexo_storage::SqliteGrantLedger::open(&paths.database) else {
                return unavailable();
            };
            match ledger.revoke_profile(&profile) {
                Ok(count) => std::iter::once(Action::McpGrantsRevoked { count })
                    .chain(mcp_audit_loaded())
                    .chain(mcp_profiles_loaded())
                    .collect(),
                Err(error) => vec![Action::McpRevokeFailed {
                    message: error.to_string(),
                }],
            }
        });
    }

    async fn delete_mcp_profile(&self, name: String) {
        self.off_the_loop(move || {
            use dexo_app::mcp::GrantLedger;
            let Ok(paths) = AppPaths::discover() else {
                return Vec::new();
            };
            let (Ok(db), Ok(ledger)) = (
                Database::open(&paths.database),
                dexo_storage::SqliteGrantLedger::open(&paths.database),
            ) else {
                return Vec::new();
            };
            let _ = ledger.revoke_profile(&name);
            match dexo_storage::McpProfileRepository::new(db.connection()).delete(&name) {
                Ok(_) => std::iter::once(Action::McpProfileDeleted { name })
                    .chain(mcp_profiles_loaded())
                    .collect(),
                Err(error) => vec![Action::McpRevokeFailed {
                    message: error.to_string(),
                }],
            }
        });
    }

    async fn revoke_all_mcp(&self) {
        self.off_the_loop(|| {
            let unavailable = || {
                vec![Action::McpRevokeFailed {
                    message: "storage unavailable".into(),
                }]
            };
            let Ok(paths) = AppPaths::discover() else {
                return unavailable();
            };
            let Ok(ledger) = dexo_storage::SqliteGrantLedger::open(&paths.database) else {
                return unavailable();
            };
            match ledger.revoke_all() {
                Ok(count) => std::iter::once(Action::McpGrantsRevoked { count })
                    .chain(mcp_audit_loaded())
                    .chain(mcp_profiles_loaded())
                    .collect(),
                Err(error) => vec![Action::McpRevokeFailed {
                    message: error.to_string(),
                }],
            }
        });
    }

    pub fn action_tx(&self) -> &tokio::sync::mpsc::Sender<Action> {
        &self.action_tx
    }
}

/// One audit event as a line a person reads: when, which profile, what it tried, and how
/// it ended in words -- not the columns of the table joined by spaces.
/// The call in one sentence, as the log reads aloud.
pub fn describe_audit(event: &dexo_app::mcp::AuditEvent) -> String {
    audit_line(event).sentence()
}

/// A call from the audit log as the Activity view lists it: the time of day, the tool by
/// its own name, and what came of it in words.
pub fn audit_line(event: &dexo_app::mcp::AuditEvent) -> crate::screens::mcp_audit::AuditLine {
    let time = chrono::DateTime::from_timestamp(event.timestamp, 0)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default();
    // `tools/call data_update` and `grant data_update` name the tool last.
    let tool = event
        .request
        .rsplit(' ')
        .next()
        .unwrap_or(&event.request)
        .to_string();
    // A status that is an error code (`POLICY_DENIED`) reads as the words it is.
    let code = !event.status.is_empty()
        && event
            .status
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch == '_');
    let words = event.status.replace('_', " ").to_lowercase();
    let outcome = match (event.decision.as_str(), code) {
        ("ask", _) => "waiting for approval".to_string(),
        ("approved", _) => "approved by a person".to_string(),
        ("deny", _) => format!("refused: {}", event.status),
        (_, true) => format!("refused: {words}"),
        _ if event.status.is_empty() || event.status == "ok" => "ok".to_string(),
        _ => event.status.clone(),
    };
    crate::screens::mcp_audit::AuditLine {
        time,
        profile: event.profile.clone(),
        client: event.client.clone(),
        tool,
        target: event.target.clone(),
        outcome,
        duration_ms: event.duration_ms,
        rows: event.rows,
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
            connection: grant.connection.clone(),
            selectors: grant
                .selectors
                .iter()
                .map(|rule| rule.selector.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            ask_secs: grant.ask_secs,
        })
        .collect()
}

/// New MCP Grant: the grant `dexo mcp grant create` would make with the same answers,
/// written where the MCP server reads it. Says what was made, or why not.
/// The MCP profiles as the Agents screen lists them, with their live grants.
fn mcp_profiles_loaded() -> Option<Action> {
    let paths = AppPaths::discover().ok()?;
    let db = Database::open(&paths.database).ok()?;
    let profiles = dexo_storage::McpProfileRepository::new(db.connection())
        .list()
        .ok()?;
    // The grants list used to be fixture-only: nothing read the ledger, so the
    // screen showed a permanently empty section.
    let ledger = dexo_storage::SqliteGrantLedger::open(&paths.database).ok();
    let now = unix_seconds();
    let profiles = profiles
        .into_iter()
        .map(|profile| crate::screens::mcp_profiles::McpProfileSummary {
            connections: profile.connections.clone(),
            raw_read: profile.query_mode == dexo_app::mcp::QueryMode::RawReadSql,
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
    Some(Action::McpProfilesLoaded { profiles })
}

/// The audit log and the writes waiting for approval.
fn mcp_audit_loaded() -> Option<Action> {
    use dexo_app::mcp::GrantLedger;
    let paths = AppPaths::discover().ok()?;
    let ledger = dexo_storage::SqliteGrantLedger::open(&paths.database).ok()?;
    let now = unix_now();
    let events = ledger
        .recent_audits(200)
        .into_iter()
        .map(|event| audit_line(&event))
        .collect();
    let pending = ledger.pending_approvals(now);
    Some(Action::McpAuditLoaded {
        events,
        pending,
        now,
    })
}

/// Each agent's config as it is now, and what an entry would run.
fn mcp_clients() -> Action {
    use dexo_app::mcp::clients::{McpClient, Places, dexo_command};
    let Ok(places) = Places::discover() else {
        return Action::McpClientsLoaded {
            clients: Vec::new(),
            command: String::new(),
            project: String::new(),
        };
    };
    let clients = McpClient::ALL
        .into_iter()
        .map(|client| crate::screens::mcp_setup::ClientRow {
            client,
            path: home_relative(&client.config_path(&places), &places.home),
            skill: client
                .skill_path(&places)
                .map(|path| home_relative(&path, &places.home)),
            state: client.state(&places),
            found: client.installed(&places),
        })
        .collect();
    Action::McpClientsLoaded {
        clients,
        command: dexo_command().unwrap_or_else(|_| "dexo".into()),
        project: home_relative(&places.project, &places.home),
    }
}

/// A path as people read it: under the home folder, from `~`.
pub(crate) fn home_relative(path: &std::path::Path, home: &std::path::Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => {
            std::path::Path::new("~").join(rest).display().to_string()
        }
        _ => path.display().to_string(),
    }
}

/// A profile's connections or SQL reading changed and saved, as `dexo mcp profile set`
/// would: what it now allows, or why not.
fn save_mcp_profile_access(
    name: &str,
    connections: Option<Vec<String>>,
    reads: Option<bool>,
) -> Result<String, String> {
    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    let db = Database::open(&paths.database).map_err(|error| error.to_string())?;
    let repo = dexo_storage::McpProfileRepository::new(db.connection());
    let mut profile = repo
        .get_by_name(name)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("{name} is not a profile any more"))?;
    if let Some(connections) = connections {
        let saved = ConnectionRepository::new(db.connection());
        for connection in &connections {
            let found = saved
                .get_by_name(connection)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("{connection} is not a saved connection"))?;
            dexo_app::mcp::McpConnection::from_profile(&found)
                .map_err(|error| format!("{connection}: {error}"))?;
        }
        profile.connections = connections;
    }
    if let Some(reads) = reads {
        profile.query_mode = if reads {
            dexo_app::mcp::QueryMode::RawReadSql
        } else {
            dexo_app::mcp::QueryMode::StructuredOnly
        };
    }
    profile.validate().map_err(|error| error.to_string())?;
    repo.save(&profile).map_err(|error| error.to_string())?;
    Ok(format!(
        "{name} uses {}, and {}.",
        profile.connections.join(", "),
        if profile.query_mode == dexo_app::mcp::QueryMode::RawReadSql {
            "reads SQL"
        } else {
            "browses only"
        }
    ))
}

/// The profile made -- or the one picked, enabled -- and the agent's config written, as
/// `dexo mcp profile create`, `set`, `allow`, `enable` and `setup` would. What was done
/// comes back in lines.
fn set_up_mcp_client(
    client: dexo_app::mcp::clients::McpClient,
    profile: crate::screens::mcp_setup::SetupProfile,
    skill: bool,
) -> Result<Vec<String>, String> {
    use crate::screens::mcp_setup::SetupProfile;
    use dexo_app::mcp::clients::{McpClient, Places, dexo_command};
    let text = |error: dexo_app::AppError| error.to_string();
    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    let db = Database::open(&paths.database).map_err(|error| error.to_string())?;
    let repo = dexo_storage::McpProfileRepository::new(db.connection());
    let (name, connections) = match profile {
        SetupProfile::New {
            name,
            connections,
            reads,
        } => {
            let saved = ConnectionRepository::new(db.connection());
            for connection in &connections {
                let found = saved
                    .get_by_name(connection)
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("{connection} is not a saved connection"))?;
                dexo_app::mcp::McpConnection::from_profile(&found)
                    .map_err(|error| format!("{connection}: {error}"))?;
            }
            let mut made = dexo_app::mcp::McpProfile::new(&name);
            made.connections = connections.clone();
            made.query_mode = if reads {
                dexo_app::mcp::QueryMode::RawReadSql
            } else {
                dexo_app::mcp::QueryMode::StructuredOnly
            };
            // The connections are the limit; every object in them may be seen.
            made.selectors = vec![
                dexo_app::mcp::SelectorRule::parse(dexo_app::mcp::Effect::Allow, "*")
                    .map_err(text)?,
            ];
            made.enabled = true;
            made.validate().map_err(text)?;
            repo.save(&made).map_err(|error| error.to_string())?;
            (name, connections)
        }
        SetupProfile::Existing(name) => {
            let mut found = repo
                .get_by_name(&name)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("{name} is not a profile any more"))?;
            if found.connections.is_empty() {
                return Err(format!(
                    "{name} uses no connection yet: make a new profile here, or give it one with `dexo mcp profile set --name {name} --connection NAME`."
                ));
            }
            if !found.enabled {
                found.enabled = true;
                repo.save(&found).map_err(|error| error.to_string())?;
            }
            (name, found.connections)
        }
    };
    let places = Places::discover().map_err(text)?;
    let command = dexo_command().map_err(|error| error.to_string())?;
    let done = client
        .set_up(&places, &command, &name, skill)
        .map_err(text)?;
    let shown = |path: &std::path::Path| home_relative(path, &places.home);
    let mut lines = vec![match &done.backup {
        Some(backup) => format!(
            "✓ {} written (the old one kept as {})",
            shown(&done.config),
            shown(backup)
        ),
        None => format!("✓ {} written", shown(&done.config)),
    }];
    if let Some(skill) = &done.skill {
        lines.push(format!("✓ {} written", shown(skill)));
    }
    lines.push(format!(
        "✓ {name} enabled, on {}, read-only",
        connections.join(", ")
    ));
    lines.push(if client == McpClient::ClaudeCode {
        "→ restart Claude Code here, and approve the project's servers".to_string()
    } else {
        format!("→ restart {}", client.name())
    });
    lines.push("→ a write needs a grant: Profiles, then g".into());
    Ok(lines)
}

fn create_mcp_grant(
    profile: &str,
    request: &dexo_app::mcp::GrantRequest,
) -> Result<String, String> {
    use dexo_app::mcp::GrantLedger;
    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    let db = Database::open(&paths.database).map_err(|error| error.to_string())?;
    let loaded = dexo_storage::McpProfileRepository::new(db.connection())
        .get_by_name(profile)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("unknown MCP profile '{profile}'"))?;
    let saved = ConnectionRepository::new(db.connection())
        .get_by_name(&request.connection)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("unknown connection '{}'", request.connection))?;
    let grant = request
        .issue(&loaded, &saved, unix_seconds())
        .map_err(|error| error.to_string())?;
    let lasts = crate::screens::mcp_profiles::duration_words(grant.expires_at - unix_seconds());
    let message = if grant.asks() {
        format!(
            "Granted {} on {} ({}): each write waits up to {} for you under Agents, Approvals; the grant ends in {lasts}.",
            grant.tools.join(", "),
            request.selector,
            request.connection,
            crate::screens::mcp_profiles::duration_words(i64::from(grant.ask_secs)),
        )
    } else {
        format!(
            "Granted {} on {} ({}) for one write; the grant ends in {lasts}.",
            grant.tools.join(", "),
            request.selector,
            request.connection,
        )
    };
    dexo_storage::SqliteGrantLedger::open(&paths.database)
        .map_err(|error| error.to_string())?
        .insert_grant(grant)
        .map_err(|error| error.to_string())?;
    Ok(message)
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

    /// The copy of a connection has references of its own: its password has to be put
    /// under them, or the copy saves and then cannot connect.
    #[test]
    fn a_copy_gets_the_password_of_the_original() {
        use super::{SessionSecrets, copy_secrets};
        use secrecy::ExposeSecret;
        let original = ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(1)),
            None,
            "pg",
            "postgres",
            "local",
            serde_json::json!({"host": "h", "port": 5432, "username": "u", "database": "d"}),
            SecretRef::new("old".into()),
        );
        let copy = ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(2)),
            None,
            "pg (copy)",
            "postgres",
            "local",
            original.config.clone(),
            SecretRef::new("new".into()),
        );
        let secrets = SessionSecrets {
            keyring: Box::new(MemorySecretStore::default()),
            memory: Arc::new(MemorySecretStore::default()),
        };
        secrets.put_keychain("old", "hunter2").unwrap();
        assert!(copy_secrets(&secrets, &original, &copy));
        assert_eq!(
            secrets.get("new").unwrap().unwrap().expose_secret(),
            "hunter2"
        );
        // The original keeps its own.
        assert_eq!(
            secrets.get("old").unwrap().unwrap().expose_secret(),
            "hunter2"
        );
    }

    /// A login the server refuses says so, apart from every other failure: it is what
    /// brings the password prompt back.
    #[tokio::test]
    async fn a_refused_login_is_told_apart_from_other_failures() {
        let profile = ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(2)),
            None,
            "shop",
            "postgres",
            "local",
            serde_json::json!({"host": "h", "port": 5432, "username": "u", "database": "d"}),
            SecretRef::new("held".into()),
        );
        let memory = MemorySecretStore::default();
        let factory: Arc<dyn ConnectionFactory> = Arc::new(Refuses);
        let error = dial(
            factory,
            &profile,
            Password::Ready(SecretString::from("wrong")),
            None,
            &memory,
            false,
        )
        .await
        .err()
        .unwrap();
        assert!(error.rejected);
        assert_eq!(error.message, "password authentication failed");
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
            let message = dial(
                Arc::clone(&factory),
                &profile,
                password,
                None,
                &memory,
                forget,
            )
            .await
            .err()
            .unwrap()
            .to_string();
            assert!(!message.contains("connect again"), "{message}");
            assert!(memory.get("held").unwrap().is_some());
        }
        let message = dial(factory, &profile, ready(), None, &memory, true)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(message.contains("connect again"), "{message}");
        assert!(memory.get("held").unwrap().is_none());
    }
}

#[cfg(test)]
mod audit_tests {
    use super::describe_audit;
    use dexo_app::mcp::AuditEvent;

    fn event(request: &str, decision: &str, target: &str, status: &str) -> AuditEvent {
        AuditEvent {
            timestamp: 0,
            request: request.into(),
            operation_id: None,
            profile: "pg-dev".into(),
            client: "test".into(),
            target: target.into(),
            decision: decision.into(),
            grant_id: None,
            duration_ms: 0,
            rows: 0,
            bytes: 0,
            status: status.into(),
            sql: None,
        }
    }

    /// `allow ... POLICY_DENIED` contradicted itself; an event is a line of words with
    /// its outcome, and no empty column left a double space.
    #[test]
    fn an_audit_event_reads_as_one_line_of_words() {
        let refused = describe_audit(&event(
            "tools/call data_update",
            "allow",
            "public.orders",
            "POLICY_DENIED",
        ));
        assert!(
            refused.ends_with("pg-dev: data_update on public.orders -- refused: policy denied"),
            "{refused}"
        );
        let ok = describe_audit(&event("tools/call list_connections", "allow", "", "ok"));
        assert!(ok.ends_with("pg-dev: list_connections -- ok"), "{ok}");
        let asking = describe_audit(&event(
            "grant data_update",
            "ask",
            "public.orders",
            "waiting",
        ));
        assert!(asking.ends_with("waiting for approval"), "{asking}");
        assert!(!refused.contains("  "), "{refused}");
    }
}
