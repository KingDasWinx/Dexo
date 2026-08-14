use crossterm::event::{KeyEvent, MouseEvent};
use dexo_app::ScriptPolicy;
use dexo_app::event::TaskId;
use dexo_driver_api::{ColumnMeta, DbValue, QueryId, QueryRequest, TransactionState};

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
    },
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
pub enum Effect {
    StartQuery(QueryRequest),
    StartScript {
        statements: Vec<String>,
        policy: ScriptPolicy,
    },
    CancelQuery(QueryId),
    PersistLayout,
    BeginTransaction,
    CommitTransaction,
    RollbackTransaction,
    Quit,
}
