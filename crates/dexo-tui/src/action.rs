use crossterm::event::{KeyEvent, MouseEvent};
use dexo_app::{ConnectionProfile, NewConnection, ScriptPolicy};
use dexo_driver_api::{ColumnMeta, DbValue, TransactionMode, TransactionState};

use std::path::PathBuf;
use std::sync::Arc;

use crate::runtime::{OperationId, OperationKey, SessionId};
use crate::screens::transfer::TransferMode;

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
        driver: String,
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
        index: usize,
        columns: Vec<ColumnMeta>,
    },
    QueryRows {
        key: crate::runtime::OperationKey,
        index: usize,
        rows: Vec<Vec<DbValue>>,
    },
    QueryNotice {
        key: crate::runtime::OperationKey,
        index: usize,
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
    Bootstrapped(Box<crate::runtime::storage_worker::BootstrapState>),
    OpenPalette,
    ClosePalette,
    PaletteQuery(String),
    PaletteSelect,
    ExecuteQuery,
    ExecuteStatement,
    ExecuteSelection,
    ExecuteDocument,
    CancelQuery,
    BeginTransaction,
    Savepoint,
    RollbackSavepoint,
    ReleaseSavepoint,
    CommitTransaction,
    RollbackTransaction,
    Focus(FocusTarget),
    ExplorerExpand,
    ExplorerCopyName,
    RefreshCatalogNode,
    RefreshCatalogSubtree,
    RefreshCatalogAll,
    CatalogLoaded {
        operation: crate::runtime::OperationId,
        session: String,
        generation: u64,
        parent: Option<dexo_driver_api::ObjectId>,
        list: dexo_driver_api::CatalogList,
        replace_roots: bool,
    },
    CatalogFailed {
        operation: crate::runtime::OperationId,
        session: String,
        generation: u64,
        parent: Option<dexo_driver_api::ObjectId>,
        message: String,
        retryable: bool,
    },
    OpenObjectInspector,
    OpenObjectDdl,
    OpenObjectData,
    OpenDependencies,
    OpenDependents,
    ExplorerUp,
    ExplorerDown,
    SwitchTab {
        index: usize,
    },
    NextTab,
    NextDocument,
    NewDocument,
    SelectGridRow,
    SelectGridColumn,
    NextResultTab,
    PrevResultTab,
    SelectResultTab {
        index: usize,
    },
    InspectorNextTab,
    NextDataPage,
    PrevDataPage,
    SaveActiveDocument,
    OpenDocument,
    CycleTheme,
    CycleKeymap,
    ToggleMouse,
    ChangeDataPage {
        offset: u64,
    },
    ApplyRemoteSort,
    ApplyRemoteFilter,
    DataPageLoaded {
        generation: u64,
        session: String,
        page: dexo_driver_api::DataPage,
    },
    DataPageFailed {
        generation: u64,
        message: String,
    },
    ValueFetched {
        generation: u64,
        bytes: Vec<u8>,
    },
    MutationsApplied {
        generation: u64,
        session: String,
    },
    MutationsFailed {
        generation: u64,
        message: String,
    },
    GoToDefinition,
    InspectorLoaded {
        generation: u64,
        session: String,
        qualified_name: String,
        object: Option<dexo_driver_api::CatalogObject>,
        ddl: Option<String>,
        dependencies: Vec<dexo_driver_api::ObjectId>,
        dependents: Vec<dexo_driver_api::ObjectId>,
        effective_privileges: Vec<String>,
        restrictions: Vec<String>,
    },
    InspectorFailed {
        generation: u64,
        message: String,
    },
    ClipboardWritten {
        text: String,
    },
    ClipboardFailed {
        message: String,
    },
    OfflineCatalogLoaded {
        generation: u64,
        list: dexo_driver_api::CatalogList,
        created_at: Option<String>,
    },
    ApplyFavorites {
        ids: Vec<String>,
    },
    CopySimpleName,
    CopyQualifiedName,
    CopyDdl,
    ToggleFavorite,
    ToggleFavoritesOnly,
    ToggleSystemObjects,
    CopyGrid(dexo_app::data::CopyFormat),
    OpenReview,
    ConfirmProduction,
    ApplyChanges,
    FailApply,
    RevertChanges,
    InspectValue,
    OpenRelated,
    DataNavBack,
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
    SchemaDiffLoaded {
        from_label: String,
        to_label: String,
        ordered: Vec<dexo_app::schema_diff::OrderedChange>,
    },
    SchemaDiffFailed {
        message: String,
    },
    SecurityLoaded {
        principals: Vec<String>,
        grants: Vec<dexo_driver_api::GrantRecord>,
    },
    SecurityFailed {
        message: String,
    },
    OpenTransfer,
    OpenBackup,
    OpenRestore,
    TransferProgress {
        operation: OperationId,
        rows: u64,
        bytes: u64,
    },
    TransferFinished {
        operation: OperationId,
        message: String,
    },
    TransferFailed {
        operation: OperationId,
        message: String,
    },
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
    McpGrantsRevoked {
        count: usize,
    },
    McpRevokeFailed {
        message: String,
    },
    OpenSettings,
    ConfirmResetSettings,
    OpenRecovery,
    ConfirmRecover,
    ConfirmDiscardRecovery,
    OpenMcpAudit,
    OpenDiagnostics,
    DiagnosticsWritten {
        path: std::path::PathBuf,
    },
    DiagnosticsFailed {
        message: String,
    },
    ResultsUp,
    ResultsDown,
    ResultsLeft,
    ResultsRight,
    ResultsPageUp,
    ResultsPageDown,
    ResultsTop,
    OpenResultsMenu,
    ToggleResultsPick,
    ResultsExtendUp,
    ResultsExtendDown,
    ToggleHelp,
    CycleLayout,
    ResetLayout,
    HideInspector,
    LayoutResultsFocus,
    GrowResults,
    ShrinkResults,
    GrowExplorer,
    ShrinkExplorer,
    GrowInspector,
    ShrinkInspector,
    RefreshSqlIntelligence,
    FormatSql,
    AcceptCompletion,
    InsertSnippet,
    SubmitParameters,
    SearchHistory,
    ClearHistory,
    HistoryLoaded(Vec<String>),
    HistoryPick,
    SnippetsLoaded(Vec<dexo_sql::Snippet>),
    SnippetPick,
    DdlPreviewed {
        sql: String,
        confirmation: dexo_app::schema::Confirmation,
        warnings: Vec<String>,
    },
    SchemaApplied {
        message: String,
    },
    ExplainLoaded {
        plan: Box<dexo_driver_api::ExplainPlan>,
    },
    AdminSessionsLoaded {
        sessions: Vec<dexo_driver_api::SessionInfo>,
        captured_at: String,
        blocking: Vec<dexo_driver_api::BlockingEdge>,
    },
    DiagnosticsReady {
        preview: String,
    },
    McpProfilesLoaded {
        profiles: Vec<crate::screens::mcp_profiles::McpProfileSummary>,
    },
    McpAuditLoaded {
        events: Vec<String>,
    },
    DocumentLoaded {
        document: String,
        content: String,
    },
    DocumentConflict {
        path: String,
    },
    OpenProjects,
    SwitchProject {
        name: String,
    },
    ProjectSwitchTarget(dexo_app::Project),
    CreateProject {
        name: String,
    },
    RenameProject {
        name: String,
    },
    DeleteProject,
    ConfirmProjectDelete,
    ConfirmSwitchDirty,
    CancelProjectSwitch,
    ProjectsLoaded(Vec<dexo_app::Project>),
    ProjectLoaded {
        project: dexo_app::Project,
        documents: Vec<(String, String)>,
        layout: Option<dexo_storage::WorkbenchLayout>,
    },
    ProjectDeleted {
        name: String,
    },
    OpenConfigTransfer,
    ExportConfig {
        path: std::path::PathBuf,
    },
    ImportConfig {
        path: std::path::PathBuf,
    },
    ConfigPreviewed {
        conflicts: Vec<String>,
        needing_secret: Vec<String>,
    },
    ConfigImported {
        needing_secret: Vec<String>,
    },
    DocumentsFlushed,
    LayoutPersisted,
    ProjectSessionsClosed,
    ProjectSwitchFailed {
        message: String,
    },
    ProjectDeletePreviewed {
        project: dexo_app::Project,
        preview: dexo_storage::ProjectDeletePreview,
    },
    ApplyConfigImport,
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
    pub project_id: Option<String>,
    pub connection_id: Option<String>,
    pub sql: String,
}

#[derive(Clone, Debug)]
pub enum TransferRequest {
    Export {
        operation: OperationId,
        path: PathBuf,
        format: dexo_app::transfer::TransferFormat,
        columns: Vec<String>,
        rows: Arc<Vec<Vec<DbValue>>>,
    },
    Import {
        operation: OperationId,
        path: PathBuf,
        format: dexo_app::transfer::TransferFormat,
        target: dexo_driver_api::QualifiedName,
        strategy: dexo_app::transfer::ErrorStrategy,
        session: SessionId,
    },
    Backup {
        operation: OperationId,
        path: PathBuf,
        session: SessionId,
    },
    Restore {
        operation: OperationId,
        path: PathBuf,
        session: SessionId,
    },
}

impl TransferRequest {
    pub fn restore(path: PathBuf, session: SessionId) -> Self {
        Self::Restore {
            operation: OperationId::new(),
            path,
            session,
        }
    }

    pub fn mode(&self) -> TransferMode {
        match self {
            Self::Export { .. } => TransferMode::Export,
            Self::Import { .. } => TransferMode::Import,
            Self::Backup { .. } => TransferMode::Backup,
            Self::Restore { .. } => TransferMode::Restore,
        }
    }

    pub fn operation(&self) -> OperationId {
        match self {
            Self::Export { operation, .. }
            | Self::Import { operation, .. }
            | Self::Backup { operation, .. }
            | Self::Restore { operation, .. } => *operation,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FlushedDocument {
    pub id: String,
    pub title: String,
    pub content: String,
    pub path: Option<std::path::PathBuf>,
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
    PreviewDdl {
        change: dexo_driver_api::SchemaChange,
        session: SessionId,
        generation: u64,
    },
    LoadSchemaDiff {
        session: SessionId,
        left: dexo_app::schema_diff::DiffSource,
        right: dexo_app::schema_diff::DiffSource,
        generation: u64,
    },
    LoadSecurity {
        session: SessionId,
        generation: u64,
    },
    ApplyDdlChange {
        change: dexo_driver_api::SchemaChange,
        typed: String,
        session: SessionId,
        generation: u64,
    },
    RunExplain {
        sql: String,
        cursor: usize,
        analyze: bool,
        session: SessionId,
        generation: u64,
    },
    LoadAdminSessions {
        session: SessionId,
        generation: u64,
    },
    AdminTerminate {
        session: SessionId,
        target: String,
    },
    LoadMcpProfiles,
    LoadConnectionProfiles,
    LoadMcpAudit,
    EnableMcpProfile {
        name: String,
    },
    RevokeMcpGrants {
        profile: String,
    },
    RevokeAllMcpGrants,
    WriteDiagnostics {
        path: std::path::PathBuf,
        bundle: dexo_app::diagnostic_service::DiagnosticBundle,
    },
    RunTransfer(TransferRequest),
    LoadSnippets,
    CheckpointRecovery(RecoveryCheckpointRequest),
    PersistHistory(PersistHistoryRequest),
    LoadHistory {
        connection_id: Option<String>,
    },
    ClearHistory {
        connection_id: String,
    },
    SwitchProject {
        name: String,
    },
    CreateProject {
        name: String,
    },
    RenameProject {
        id: String,
        name: String,
    },
    DeleteProject {
        id: String,
        delete_connections: bool,
    },
    PreviewProjectDelete {
        id: String,
    },
    LoadProject {
        id: String,
    },
    ListProjects,
    ExportConfig {
        path: std::path::PathBuf,
    },
    ImportConfig {
        path: std::path::PathBuf,
    },
    ApplyConfigImport {
        path: std::path::PathBuf,
        resolutions: std::collections::HashMap<String, dexo_storage::ImportResolution>,
    },
    FlushDocuments {
        project_id: String,
        documents: Vec<FlushedDocument>,
    },
    CloseProjectSessions,
    LoadCatalogChildren {
        parent: Option<dexo_driver_api::ObjectId>,
        operation: OperationId,
        session: SessionId,
        generation: u64,
        replace_roots: bool,
        include_system: bool,
    },
    LoadObjectInspector {
        id: dexo_driver_api::ObjectId,
        session: SessionId,
        generation: u64,
    },
    LoadTableData {
        request: dexo_driver_api::DataRequest,
        session: SessionId,
        generation: u64,
    },
    FetchValue {
        value: dexo_driver_api::RemoteValueRef,
        offset: u64,
        limit: u32,
        session: SessionId,
        generation: u64,
    },
    ApplyMutations {
        mutations: Vec<dexo_driver_api::Mutation>,
        session: SessionId,
        generation: u64,
    },
    CopyToClipboard {
        text: String,
    },
    CaptureCatalogSnapshot {
        connection_id: String,
        database_name: String,
        session: SessionId,
        include_system: bool,
    },
    LoadOfflineCatalog {
        connection_id: String,
        database_name: String,
        generation: u64,
    },
    LoadObjectUsage {
        project_id: String,
        connection_id: String,
    },
    PersistFavorite {
        project_id: String,
        connection_id: String,
        object_id: String,
        favorite: bool,
    },
    Shutdown,
    Quit,
}
