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
                let sql = preview.plan.sqls().collect::<Vec<_>>().join(";\n");
                let _ = tx
                    .send(crate::action::Action::DdlPreviewed {
                        sql,
                        confirmation: preview.confirmation,
                        warnings: preview.plan.warnings,
                    })
                    .await;
            }
            Err(message) => {
                let _ = tx
                    .send(crate::action::Action::OperationFailed {
                        key: crate::runtime::OperationKey::new(
                            crate::runtime::OperationId::new(),
                            "",
                            "",
                            0,
                        ),
                        message,
                    })
                    .await;
            }
        },
        Err(message) => {
            let _ = tx
                .send(crate::action::Action::OperationFailed {
                    key: crate::runtime::OperationKey::new(
                        crate::runtime::OperationId::new(),
                        "",
                        "",
                        0,
                    ),
                    message,
                })
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
    let Ok(manager) = manager_for(session, &session_id) else {
        let _ = tx
            .send(crate::action::Action::SchemaApplied {
                message: "driver has no DDL".into(),
            })
            .await;
        return;
    };
    match manager.preview_schema(&session_id, change).await {
        Ok(preview) => {
            let answer = if typed.is_empty() {
                ConfirmationAnswer::None
            } else {
                ConfirmationAnswer::Text(typed)
            };
            match manager.apply_schema(preview.operation_id, answer).await {
                Ok(outcome) => {
                    let _ = tx
                        .send(crate::action::Action::SchemaApplied {
                            message: format!("ddl {outcome:?}"),
                        })
                        .await;
                }
                Err(message) => {
                    let _ = tx
                        .send(crate::action::Action::OperationFailed {
                            key: crate::runtime::OperationKey::new(
                                crate::runtime::OperationId::new(),
                                "",
                                "",
                                0,
                            ),
                            message,
                        })
                        .await;
                }
            }
        }
        Err(message) => {
            let _ = tx
                .send(crate::action::Action::OperationFailed {
                    key: crate::runtime::OperationKey::new(
                        crate::runtime::OperationId::new(),
                        "",
                        "",
                        0,
                    ),
                    message,
                })
                .await;
        }
    }
}

pub async fn diff_live(
    session: std::sync::Arc<dyn dexo_driver_api::Session>,
    session_id: String,
    left: DiffSource,
    right: DiffSource,
    tx: tokio::sync::mpsc::Sender<crate::action::Action>,
) {
    let from_label = format!("{left:?}");
    let to_label = format!("{right:?}");
    let result = async {
        let manager = manager_for(Arc::clone(&session), &session_id)?;
        hydrate_source(&manager, session.as_ref(), &left).await?;
        hydrate_source(&manager, session.as_ref(), &right).await?;
        manager
            .diff(DiffRequest {
                left,
                right,
                filters: DiffFilters::all(),
                renames: vec![],
            })
            .await
    }
    .await;
    match result {
        Ok(outcome) => {
            let _ = tx
                .send(crate::action::Action::SchemaDiffLoaded {
                    from_label,
                    to_label,
                    ordered: outcome.ordered,
                })
                .await;
        }
        Err(message) => {
            let _ = tx
                .send(crate::action::Action::SchemaDiffFailed { message })
                .await;
        }
    }
}

async fn hydrate_source(
    manager: &SchemaManager,
    session: &dyn dexo_driver_api::Session,
    source: &DiffSource,
) -> Result<(), String> {
    let DiffSource::Live(id) = source else {
        return Ok(());
    };
    let catalog = session
        .catalog()
        .ok_or_else(|| "catalog is unavailable".to_string())?;
    let list = catalog
        .list_children(None, &dexo_driver_api::CatalogListOptions::default())
        .await
        .map_err(|error| error.to_string())?;
    manager.put_live(
        id.clone(),
        SchemaSnapshot::capture("postgres", "0", "now", id.clone(), list.objects),
    );
    Ok(())
}
