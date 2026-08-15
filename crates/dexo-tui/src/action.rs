use crossterm::event::{KeyEvent, MouseEvent};
use dexo_app::{ConnectionProfile, NewConnection, ScriptPolicy};
use dexo_driver_api::{ColumnMeta, DbValue, TransactionMode, TransactionState};

use crate::runtime::{OperationId, OperationKey, SessionId};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize {
        width: u16,
        height: u16,
    },
    ConnectionChanged {
        name: String,
        ready: bool,
        environment: String,
        session: Option<crate::runtime::SessionId>,
        generation: u64,
        token: u64,
        read_only: bool,
    },
    OpenConnectionForm,
    ConnectionFormError {
        message: String,
    },
    SecretRequired {
        purpose: crate::screens::secret_prompt::SecretPurpose,
        profile: ConnectionProfile,
        buffer: crate::screens::secret_prompt::SecretBuffer,
    },
    SubmitSecret {
        kind: crate::screens::secret_prompt::SecretChoiceKind,
    },
    ConfirmDeleteProfile {
        decision: crate::screens::secret_prompt::DeleteSecretDecision,
    },
    OpenConnections,
    ConnectSelected,
    DuplicateConnection,
    TestConnection,
    DeleteConnection,
    MoveConnectionGroup {
        group: String,
    },
    CloseSelectedSession,
    ProfilesLoaded(Vec<ConnectionProfile>),
    ConnectionTested {
        name: String,
        ok: bool,
        message: String,
    },
    ProfileSaved(ConnectionProfile),
    ProfileDeleted {
        name: String,
    },
    SessionClosed {
        session: crate::runtime::SessionId,
    },
    SaveConnection,
    QueryResultSetStarted {
        key: crate::runtime::OperationKey,
        index: usize,
    },
    QueryMeta {
        key: crate::runtime::OperationKey,
        columns: Vec<ColumnMeta>,
    },
    QueryRows {
        key: crate::runtime::OperationKey,
        rows: Vec<Vec<DbValue>>,
    },
    QueryNotice {
        key: crate::runtime::OperationKey,
        message: String,
    },
    QueryResultSetFinished {
        key: crate::runtime::OperationKey,
        index: usize,
        rows_affected: Option<u64>,
    },
    ScriptFinished {
        key: crate::runtime::OperationKey,
    },
    CheckpointTick,
    TransactionChanged {
        session: crate::runtime::SessionId,
        generation: u64,
        state: TransactionState,
    },
    OperationStarted(OperationKey),
    OperationFailed {
        key: OperationKey,
        message: String,
    },
    OperationCancelled(OperationKey),
    Bootstrapped(crate::runtime::storage_worker::BootstrapState),
    OpenPalette,
    ClosePalette,
    PaletteQuery(String),
    PaletteSelect,
    ExecuteQuery,
    CancelQuery,
    BeginTransaction,
    Savepoint,
    CommitTransaction,
    RollbackTransaction,
    Focus(FocusTarget),
    ExplorerExpand,
    ExplorerCopyName,
    CopyGrid(dexo_app::data::CopyFormat),
    OpenReview,
    ConfirmProduction,
    ApplyChanges,
    FailApply,
    RevertChanges,
    InspectValue,
    OpenRelated,
    OpenDdlPreview,
    ConfirmDdl,
    ApplyDdl,
    ApplyRawDdl,
    OpenSecurity,
    SchemaFocusNext,
    OpenSchemaDiff,
    SchemaDiffToggleAdded,
    SchemaDiffToggleRemoved,
    SchemaDiffToggleChanged,
    ConfirmSchemaDiff,
    ApplySchemaDiff,
    OpenTransfer,
    OpenBackup,
    OpenRestore,
    OpenExplain,
    ExplainViewTree,
    ExplainViewTable,
    ExplainViewSummary,
    ConfirmExplainAnalyze,
    OpenAdmin,
    AdminPause,
    AdminResume,
    ConfirmAdmin,
    OpenMcpProfiles,
    ConfirmMcpEnable,
    RevokeAllMcpGrants,
    OpenSettings,
    ConfirmResetSettings,
    OpenRecovery,
    ConfirmRecover,
    ConfirmDiscardRecovery,
    OpenMcpAudit,
    OpenDiagnostics,
    ResultsUp,
    ResultsDown,
    ResultsLeft,
    ResultsRight,
    ResultsPageUp,
    ResultsPageDown,
    ResultsTop,
    RefreshSqlIntelligence,
    FormatSql,
    AcceptCompletion,
    InsertSnippet,
    SubmitParameters,
    SearchHistory,
    ClearHistory,
    HistoryLoaded(Vec<String>),
    SnippetsLoaded(Vec<dexo_sql::Snippet>),
    DocumentLoaded {
        document: String,
        content: String,
    },
    DocumentConflict {
        path: String,
    },
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusTarget {
    Explorer,
    Editor,
    Results,
    Inspector,
}

#[derive(Clone, Debug)]
pub struct ScriptRequest {
    pub key: OperationKey,
    pub statements: Vec<String>,
    pub policy: ScriptPolicy,
    pub parameters: Vec<DbValue>,
    pub timeout: std::time::Duration,
}

#[derive(Clone, Debug)]
pub struct DocumentIoRequest {
    pub document: String,
    pub path: std::path::PathBuf,
    pub content: String,
    pub expected_fingerprint: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RecoveryCheckpointRequest {
    pub document: String,
    pub project_id: String,
    pub title: String,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct PersistHistoryRequest {
    pub connection_id: Option<String>,
    pub sql: String,
}

#[derive(Clone, Debug)]
pub enum Effect {
    StartScript(ScriptRequest),
    CancelOperation(OperationId),
    PersistLayout {
        project_id: String,
        layout: dexo_storage::WorkbenchLayout,
    },
    CreateConnection {
        input: NewConnection,
        password: String,
    },
    ConnectProfile {
        profile: ConnectionProfile,
        token: u64,
    },
    SubmitSecret {
        kind: crate::screens::secret_prompt::SecretChoiceKind,
        profile: ConnectionProfile,
        secret: crate::screens::secret_prompt::SecretBuffer,
    },
    DuplicateProfile {
        id: dexo_app::ConnectionId,
    },
    TestConnection {
        input: NewConnection,
        password: String,
    },
    TestSavedProfile {
        profile: ConnectionProfile,
    },
    SaveProfile {
        profile: ConnectionProfile,
    },
    DeleteProfile {
        profile: ConnectionProfile,
        delete_secrets: bool,
    },
    MoveProfileGroup {
        id: dexo_app::ConnectionId,
        group_path: Option<String>,
    },
    CloseSession {
        session: SessionId,
    },
    BeginTransaction {
        session: SessionId,
        mode: TransactionMode,
    },
    CommitTransaction {
        session: SessionId,
    },
    RollbackTransaction {
        session: SessionId,
    },
    Savepoint {
        session: SessionId,
        name: String,
    },
    RollbackToSavepoint {
        session: SessionId,
        name: String,
    },
    ReleaseSavepoint {
        session: SessionId,
        name: String,
    },
    LoadDocument(DocumentIoRequest),
    SaveDocument(DocumentIoRequest),
    CheckpointRecovery(RecoveryCheckpointRequest),
    PersistHistory(PersistHistoryRequest),
    LoadHistory {
        connection_id: Option<String>,
    },
    ClearHistory {
        connection_id: String,
    },
    Shutdown,
    Quit,
}
