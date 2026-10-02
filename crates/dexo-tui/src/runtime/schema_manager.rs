use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use dexo_app::schema::{
    ApplyRequest, CacheAction, CatalogScope, Confirmation, ConfirmationAnswer, DdlPolicy,
    apply_change, invalidate_after_ddl, preview_change, production_policy,
};
use dexo_app::schema_diff::{
    DiffSource, OrderedChange, SchemaDifference, SchemaSnapshot, plan_migration,
};
use dexo_driver_api::{DdlExecutor, DdlOutcome, DdlPlan, ObjectId, SchemaChange};
use sha2::{Digest, Sha256};

use crate::runtime::OperationId;

#[derive(Clone, Debug)]
pub struct SchemaPreview {
    pub operation_id: OperationId,
    pub confirmation: Confirmation,
    pub plan: DdlPlan,
    pub change: SchemaChange,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MigrationOperation {
    pub operation_id: OperationId,
    pub fingerprint: String,
    pub statements: Vec<String>,
    pub completed: Vec<usize>,
    pub failed: Option<usize>,
    pub catalog_state: CacheAction,
    pub remainder: Option<String>,
}

pub struct SchemaManager {
    executor: Arc<dyn DdlExecutor>,
    policy: DdlPolicy,
    read_only: bool,
    session: String,
    previews: Mutex<HashMap<OperationId, SchemaPreview>>,
    invalidations: Mutex<Vec<CatalogScope>>,
    ddl_calls: AtomicUsize,
    snapshots: Mutex<HashMap<String, SchemaSnapshot>>,
    live: Mutex<HashMap<String, SchemaSnapshot>>,
}

impl SchemaManager {
    pub fn new(executor: Arc<dyn DdlExecutor>, session: impl Into<String>) -> Self {
        Self {
            executor,
            policy: production_policy(),
            read_only: false,
            session: session.into(),
            previews: Mutex::new(HashMap::new()),
            invalidations: Mutex::new(Vec::new()),
            ddl_calls: AtomicUsize::new(0),
            snapshots: Mutex::new(HashMap::new()),
            live: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
        self.policy.read_only = read_only;
    }

    pub fn put_snapshot(&self, id: impl Into<String>, snapshot: SchemaSnapshot) {
        self.snapshots
            .lock()
            .expect("schema snapshots")
            .insert(id.into(), snapshot);
    }

    pub fn put_live(&self, session: impl Into<String>, snapshot: SchemaSnapshot) {
        self.live
            .lock()
            .expect("live snapshots")
            .insert(session.into(), snapshot);
    }

    pub async fn preview_schema(
        &self,
        session_id: &str,
        change: SchemaChange,
    ) -> Result<SchemaPreview, String> {
        if session_id != self.session {
            return Err("session mismatch".into());
        }
        if self.read_only {
            return Err("connection is read-only".into());
        }
        let plan = self
            .executor
            .plan_change(&change)
            .map_err(|error| error.to_string())?;
        let preview = preview_change(&change, plan.clone(), Vec::new(), Vec::new(), &self.policy);
        let op = SchemaPreview {
            operation_id: OperationId::new(),
            confirmation: preview.confirmation,
            plan,
            change,
        };
        self.previews
            .lock()
            .expect("schema previews")
            .insert(op.operation_id, op.clone());
        Ok(op)
    }

    pub async fn apply_schema(
        &self,
        operation_id: OperationId,
        answer: ConfirmationAnswer,
    ) -> Result<DdlOutcome, String> {
        let preview = self
            .previews
            .lock()
            .expect("schema previews")
            .remove(&operation_id)
            .ok_or_else(|| "unknown schema operation".to_string())?;
        let typed = match answer {
            ConfirmationAnswer::Text(text) => Some(text),
            ConfirmationAnswer::None => None,
        };
        let outcome = apply_change(
            self.executor.as_ref(),
            ApplyRequest {
                change: &preview.change,
                plan: &preview.plan,
                policy: &self.policy,
                typed_confirmation: typed.as_deref(),
                cancelled: false,
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        self.ddl_calls.fetch_add(1, Ordering::SeqCst);
        let action = invalidate_after_ddl(outcome, preview.change.target());
        match action {
            CacheAction::InvalidateSubtree => {
                self.invalidations
                    .lock()
                    .expect("invalidations")
                    .push(CatalogScope::Table(ObjectId::new(
                        preview.change.target().object(),
                    )));
            }
            CacheAction::MarkUncertain => {
                self.invalidations
                    .lock()
                    .expect("invalidations")
                    .push(CatalogScope::Connection);
            }
            CacheAction::Keep => {}
        }
        Ok(outcome)
    }

    pub fn ddl_calls(&self) -> usize {
        self.ddl_calls.load(Ordering::SeqCst)
    }

    pub fn invalidations(&self) -> Vec<CatalogScope> {
        self.invalidations.lock().expect("invalidations").clone()
    }

    pub async fn diff(&self, request: DiffRequest) -> Result<DiffOutcome, String> {
        let left = self.load_source(&request.left)?;
        let right = self.load_source(&request.right)?;
        if left.driver != right.driver {
            return Err(format!(
                "cannot compare {} snapshot with {} snapshot",
                left.driver, right.driver
            ));
        }
        left.verify().map_err(|error| error.to_string())?;
        right.verify().map_err(|error| error.to_string())?;
        let (changes, ordered, _) = plan_migration(&left, &right, &request.renames, |change| {
            self.executor
                .plan_change(change)
                .map_err(|error| error.to_string())
        });
        Ok(DiffOutcome {
            changes,
            ordered,
            fingerprint: fingerprint_of(&left, &right),
        })
    }

    pub async fn apply_diff(
        &self,
        ordered: &[OrderedChange],
        fail_at: Option<usize>,
    ) -> MigrationOperation {
        let mut completed = Vec::new();
        let mut failed = None;
        let mut statements = Vec::new();
        for (index, item) in ordered.iter().enumerate() {
            let change = dexo_app::schema_diff::script::to_change(&item.difference);
            let plan = self.executor.plan_change(&change).unwrap_or_default();
            for sql in plan.sqls() {
                statements.push(sql.to_string());
            }
            let id = index + 1;
            if fail_at == Some(id) {
                failed = Some(id);
                break;
            }
            completed.push(id);
            let _ = apply_change(
                self.executor.as_ref(),
                ApplyRequest {
                    change: &change,
                    plan: &plan,
                    policy: &self.policy,
                    typed_confirmation: None,
                    cancelled: false,
                },
            )
            .await;
        }
        let catalog_state = if failed.is_some() {
            CacheAction::MarkUncertain
        } else {
            CacheAction::InvalidateSubtree
        };
        if catalog_state == CacheAction::MarkUncertain {
            self.invalidations
                .lock()
                .expect("invalidations")
                .push(CatalogScope::Connection);
        }
        let remainder = failed
            .map(|_| "-- remainder requires review; not a safe automatic resume\n".to_string());
        MigrationOperation {
            operation_id: OperationId::new(),
            fingerprint: String::new(),
            statements,
            completed,
            failed,
            catalog_state,
            remainder,
        }
    }

    fn load_source(&self, source: &DiffSource) -> Result<SchemaSnapshot, String> {
        match source {
            DiffSource::Live(session) => self
                .live
                .lock()
                .expect("live snapshots")
                .get(session)
                .cloned()
                .ok_or_else(|| "live source is unavailable".into()),
            DiffSource::SavedSnapshot(id) => self
                .snapshots
                .lock()
                .expect("schema snapshots")
                .get(id)
                .cloned()
                .ok_or_else(|| "saved snapshot not found".into()),
            DiffSource::JsonFile(path) => {
                let json = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
                dexo_storage::SchemaSnapshotStore::load_json(&json)
                    .map_err(|error| error.to_string())
            }
        }
    }
}

pub struct DiffRequest {
    pub left: DiffSource,
    pub right: DiffSource,
    pub filters: DiffFilters,
    pub renames: Vec<dexo_app::schema_diff::RenameMapping>,
}

#[derive(Clone, Debug, Default)]
pub struct DiffFilters {
    pub all: bool,
}

impl DiffFilters {
    pub fn all() -> Self {
        Self { all: true }
    }
}

pub struct DiffOutcome {
    pub changes: Vec<SchemaDifference>,
    pub ordered: Vec<OrderedChange>,
    pub fingerprint: String,
}

fn fingerprint_of(left: &SchemaSnapshot, right: &SchemaSnapshot) -> String {
    let mut hasher = Sha256::new();
    hasher.update(left.digest.as_bytes());
    hasher.update(right.digest.as_bytes());
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut out, byte| {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
            out
        })
}

struct SessionDdl(std::sync::Arc<dyn dexo_driver_api::Session>);

#[async_trait::async_trait]
impl DdlExecutor for SessionDdl {
    fn plan_change(&self, change: &SchemaChange) -> Result<DdlPlan, dexo_driver_api::DriverError> {
        self.0
            .ddl()
            .ok_or_else(|| dexo_driver_api::DriverError::unsupported("ddl unavailable"))?
            .plan_change(change)
    }

    async fn apply_ddl(&self, plan: &DdlPlan) -> Result<DdlOutcome, dexo_driver_api::DriverError> {
        match self.0.ddl() {
            Some(ddl) => ddl.apply_ddl(plan).await,
            None => Err(dexo_driver_api::DriverError::unsupported("ddl unavailable")),
        }
    }
}

pub fn manager_for(
    session: std::sync::Arc<dyn dexo_driver_api::Session>,
    session_id: impl Into<String>,
) -> Result<SchemaManager, String> {
    if session.ddl().is_none() {
        return Err("driver has no DDL".into());
    }
    Ok(SchemaManager::new(
        Arc::new(SessionDdl(session)),
        session_id,
    ))
}

pub async fn preview_live(
    session: std::sync::Arc<dyn dexo_driver_api::Session>,
    session_id: String,
    change: SchemaChange,
    tx: tokio::sync::mpsc::Sender<crate::action::Action>,
) {
    match manager_for(session, &session_id) {
        Ok(manager) => match manager.preview_schema(&session_id, change).await {
            Ok(preview) => {
                let _ = tx
                    .send(crate::action::Action::DdlPreviewed {
                        statements: preview.plan.sqls().map(str::to_string).collect(),
                        confirmation: preview.confirmation,
                        risk: preview.plan.risk,
                        warnings: preview.plan.warnings,
                    })
                    .await;
            }
            Err(message) => {
                let _ = tx
                    .send(crate::action::Action::SchemaFailed { message })
                    .await;
            }
        },
        Err(message) => {
            let _ = tx
                .send(crate::action::Action::SchemaFailed { message })
                .await;
        }
    }
}

pub async fn apply_live(
    session: std::sync::Arc<dyn dexo_driver_api::Session>,
    session_id: String,
    change: SchemaChange,
    typed: String,
    tx: tokio::sync::mpsc::Sender<crate::action::Action>,
) {
    let fail = |message: String| crate::action::Action::SchemaFailed {
        message: format!("Could not apply the change: {message}"),
    };
    let Ok(manager) = manager_for(session, &session_id) else {
        let _ = tx
            .send(fail("this driver has no schema changes".into()))
            .await;
        return;
    };
    let preview = match manager.preview_schema(&session_id, change.clone()).await {
        Ok(preview) => preview,
        Err(message) => {
            let _ = tx.send(fail(message)).await;
            return;
        }
    };
    let answer = if typed.is_empty() {
        ConfirmationAnswer::None
    } else {
        ConfirmationAnswer::Text(typed)
    };
    let action = match manager.apply_schema(preview.operation_id, answer).await {
        Ok(outcome) => {
            let (message, ok) = applied_message(&change, outcome);
            crate::action::Action::SchemaApplied {
                message,
                refresh: Some(change.target().clone()),
                ok,
            }
        }
        Err(message) => fail(message),
    };
    let _ = tx.send(action).await;
}

/// What a finished change says, in words, and whether it all went through. The toast read
/// `ddl Committed`, the Rust name of the outcome.
pub fn applied_message(change: &SchemaChange, outcome: DdlOutcome) -> (String, bool) {
    let target = change.target().display_unquoted();
    match outcome {
        DdlOutcome::Committed => (
            match change {
                SchemaChange::CreateTable { .. } => format!("Created {target}."),
                SchemaChange::AlterTable { .. } => format!("Altered {target}."),
                SchemaChange::CreateView { .. } => format!("Created view {target}."),
                SchemaChange::AlterRoutine { .. } => format!("Saved {target}."),
                SchemaChange::CreateIndex { .. } => format!("Created an index on {target}."),
                SchemaChange::DropObject { .. } => format!("Dropped {target}."),
                SchemaChange::RenameObject { new_name, .. } => {
                    format!("Renamed {target} to {}.", new_name.object())
                }
                SchemaChange::Grant { def, .. } => format!(
                    "Granted {} on {target} to {}.",
                    def.privileges.join(", "),
                    def.principal.object()
                ),
                SchemaChange::Revoke { def, .. } => format!(
                    "Revoked {} on {target} from {}.",
                    def.privileges.join(", "),
                    def.principal.object()
                ),
            },
            true,
        ),
        DdlOutcome::RolledBack => (
            format!("Nothing was changed on {target}: the change was rolled back."),
            false,
        ),
        DdlOutcome::PartiallyCommitted { committed } => (
            format!(
                "Only {committed} of the statements ran on {target} before one failed; check what it holds."
            ),
            false,
        ),
        DdlOutcome::Unknown => (
            format!(
                "Could not tell whether the change to {target} went through; refresh the catalog to see."
            ),
            false,
        ),
    }
}

/// The names of the saved schema snapshots, for the picker.
pub async fn list_sources(tx: tokio::sync::mpsc::Sender<crate::action::Action>) {
    let listed = tokio::task::spawn_blocking(|| {
        let paths = dexo_storage::AppPaths::discover().ok()?;
        let db = dexo_storage::Database::open(&paths.database).ok()?;
        dexo_storage::SchemaSnapshotStore::new(db.connection())
            .list()
            .ok()
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    let _ = tx
        .send(crate::action::Action::SchemaSourcesLoaded(
            listed
                .into_iter()
                .map(|info| (info.name, info.driver))
                .collect(),
        ))
        .await;
}

type Side = (
    crate::action::DiffSide,
    Option<std::sync::Arc<dyn dexo_driver_api::Session>>,
);

/// What a side is called in the dialog.
fn side_label(side: &crate::action::DiffSide) -> String {
    match side {
        crate::action::DiffSide::Live { name, .. } => name.clone(),
        crate::action::DiffSide::Snapshot { name } => format!("snapshot {name}"),
        crate::action::DiffSide::File { path } => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
    }
}

/// The snapshot a side stands for: a live connection's whole catalog as of now, a saved
/// snapshot by name, a file read and checked.
async fn snapshot_of(side: &Side) -> Result<SchemaSnapshot, String> {
    use crate::action::DiffSide;
    match (&side.0, &side.1) {
        (DiffSide::Live { driver, name, .. }, Some(session)) => {
            let reader = session
                .catalog()
                .ok_or_else(|| format!("{name} has no catalog to read"))?;
            let objects = dexo_app::CatalogService::collect_objects(reader, None)
                .await
                .map_err(|error| error.to_string())?;
            Ok(SchemaSnapshot::capture(
                driver.clone(),
                String::new(),
                chrono::Local::now().to_rfc3339(),
                name.clone(),
                objects,
            ))
        }
        (DiffSide::Live { name, .. }, None) => Err(format!(
            "{name} is no longer connected; connect it and compare again."
        )),
        (DiffSide::Snapshot { name }, _) => {
            let name = name.clone();
            tokio::task::spawn_blocking(move || {
                let paths = dexo_storage::AppPaths::discover().map_err(|e| e.to_string())?;
                let db =
                    dexo_storage::Database::open(&paths.database).map_err(|e| e.to_string())?;
                dexo_storage::SchemaSnapshotStore::new(db.connection())
                    .load_by_name(&name)
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("There is no snapshot named {name}."))
            })
            .await
            .map_err(|error| error.to_string())?
        }
        (DiffSide::File { path }, _) => {
            let json = tokio::fs::read_to_string(path)
                .await
                .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
            dexo_storage::SchemaSnapshotStore::load_json(&json)
                .map_err(|_| format!("{} is not a Dexo schema snapshot.", path.display()))
        }
    }
}

/// Reads both sides, and sends what the second has that the first lacks, in the order
/// the script has to run, or why it could not.
pub async fn compare(
    left: Side,
    right: Side,
    render: Option<std::sync::Arc<dyn dexo_driver_api::Session>>,
    tx: tokio::sync::mpsc::Sender<crate::action::Action>,
) {
    let (from_label, to_label) = (side_label(&left.0), side_label(&right.0));
    let result = async {
        let from = snapshot_of(&left).await?;
        let to = snapshot_of(&right).await?;
        if from.driver != to.driver {
            return Err(format!(
                "A {} schema cannot be compared with a {} one.",
                from.driver, to.driver
            ));
        }
        // The script is written in the dialect of a connection that is open; with none,
        // plainly.
        let (_, ordered, _) = plan_migration(&from, &to, &[], |change| match &render {
            Some(session) => session
                .ddl()
                .ok_or_else(|| "this connection has no schema changes".to_string())?
                .plan_change(change)
                .map_err(|error| error.to_string()),
            None => dexo_app::schema_diff::render_unquoted(change),
        });
        Ok(ordered)
    }
    .await;
    let action = match result {
        Ok(ordered) => crate::action::Action::SchemaDiffLoaded {
            from_label,
            to_label,
            ordered,
        },
        Err(message) => crate::action::Action::SchemaDiffFailed { message },
    };
    let _ = tx.send(action).await;
}
