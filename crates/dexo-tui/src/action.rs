use crossterm::event::{KeyEvent, MouseEvent};
use dexo_app::event::TaskId;
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
    },
    OpenConnectionForm,
    ConnectionFormError {
        message: String,
    },
    SaveConnection,
    QueryMeta {
        task: TaskId,
        columns: Vec<ColumnMeta>,
    },
    QueryRows {
        task: TaskId,
        rows: Vec<Vec<DbValue>>,
    },
    QueryMessage {
        task: TaskId,
        message: String,
    },
    QueryFinished {
        task: TaskId,
        rows_affected: Option<u64>,
    },
    TransactionChanged(TransactionState),
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
    PersistLayout,
    CreateConnection {
        input: NewConnection,
        password: String,
    },
    ConnectProfile {
        profile: ConnectionProfile,
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
    Shutdown,
    Quit,
}
