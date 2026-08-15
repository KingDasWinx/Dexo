use std::time::Duration;

use dexo_app::{DriverRegistry, Project, ProjectId};
use dexo_driver_api::TransactionState;
use dexo_storage::{
    ConnectionRepository, Database, DocumentRepository, ImportResolution, ProjectRepository,
    export_portable, import_portable_resolved, preview_import,
};
use dexo_tui::action::{Action, Effect};
use dexo_tui::model::Model;
use dexo_tui::runtime::storage_worker::StorageWorker;
use dexo_tui::runtime::{WorkbenchRuntime, project_manager::ProjectSwitchStage};
use dexo_tui::update;

struct ProjectHarness {
    dir: tempfile::TempDir,
    db_path: std::path::PathBuf,
    model: Model,
    runtime: WorkbenchRuntime,
    rx: tokio::sync::mpsc::Receiver<Action>,
}

impl ProjectHarness {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("dexo.db");
        {
            let db = Database::open(&db_path).unwrap();
            let repo = ProjectRepository::new(db.connection());
            repo.save(&Project {
                id: ProjectId(uuid::Uuid::new_v4()),
                name: "Project A".into(),
                created_at: "1".into(),
            })
            .unwrap();
            repo.save(&Project {
                id: ProjectId(uuid::Uuid::new_v4()),
                name: "Project B".into(),
                created_at: "2".into(),
            })
            .unwrap();
        }
        let worker = StorageWorker::start(db_path.clone()).unwrap();
        let bootstrap = worker.bootstrap().await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let runtime = WorkbenchRuntime::new(tx, worker, DriverRegistry::new());
        let mut model = Model::default();
        let _ = update(&mut model, Action::Bootstrapped(Box::new(bootstrap)));
        Self {
            dir,
            db_path,
            model,
            runtime,
            rx,
        }
    }

    async fn dirty_active_document(&mut self, sql: &str) {
        self.model.active_document_mut().sql.insert(0, sql).unwrap();
    }

    async fn switch_to(&mut self, name: &str) -> anyhow::Result<()> {
        let effects = update(
            &mut self.model,
            Action::SwitchProject {
                name: name.to_string(),
            },
        );
        self.dispatch(effects).await;
        if self
            .model
            .projects
            .pending
            .as_ref()
            .is_some_and(|switch| switch.stage == ProjectSwitchStage::ConfirmDirty)
        {
            let effects = update(&mut self.model, Action::ConfirmSwitchDirty);
            self.dispatch(effects).await;
        }
        self.pump_until(|model| model.project == name && model.projects.pending.is_none())
            .await;
        if self.model.project != name {
            anyhow::bail!("still on {}", self.model.project);
        }
        Ok(())
    }

    async fn dispatch(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            self.runtime.dispatch(effect).await;
        }
    }

    async fn pump_until(&mut self, done: impl Fn(&Model) -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while !done(&self.model) && tokio::time::Instant::now() < deadline {
            if let Ok(Some(action)) =
                tokio::time::timeout(Duration::from_millis(50), self.rx.recv()).await
            {
                let effects = update(&mut self.model, action);
                self.dispatch(effects).await;
            }
        }
    }

    async fn stored_document(&self, project: &str) -> String {
        let _keep = &self.dir;
        let db = Database::open(&self.db_path).unwrap();
        let project = ProjectRepository::new(db.connection())
            .get_by_name(project)
            .unwrap()
            .unwrap();
        DocumentRepository::new(db.connection())
            .list_for_project(&project.id.0.to_string())
            .unwrap()
            .into_iter()
            .next()
            .map(|document| document.content)
            .unwrap_or_default()
    }

    fn model(&self) -> &Model {
        &self.model
    }
}

#[tokio::test]
async fn switching_flushes_old_project_before_loading_new_project() {
    let mut harness = ProjectHarness::new().await;
    harness.dirty_active_document("select 42").await;
    harness.switch_to("Project B").await.unwrap();
    assert_eq!(harness.stored_document("Project A").await, "select 42");
    assert_eq!(harness.model().project, "Project B");
}

#[test]
fn switching_with_open_transaction_keeps_old_project() {
    let mut model = Model {
        project: "Project A".into(),
        transaction: TransactionState::Active,
        ..Model::default()
    };
    model.projects.load(vec![
        Project {
            id: ProjectId(uuid::Uuid::nil()),
            name: "Project A".into(),
            created_at: "1".into(),
        },
        Project {
            id: ProjectId(uuid::Uuid::new_v4()),
            name: "Project B".into(),
            created_at: "2".into(),
        },
    ]);
    let effects = update(
        &mut model,
        Action::SwitchProject {
            name: "Project B".into(),
        },
    );
    assert!(effects.is_empty());
    assert_eq!(model.project, "Project A");
    assert!(
        model
            .messages
            .iter()
            .any(|message| message.contains("transaction"))
    );
}

#[test]
fn switching_storage_failure_keeps_old_project() {
    let mut model = Model {
        project: "Project A".into(),
        ..Model::default()
    };
    model.projects.pending = Some(dexo_tui::runtime::project_manager::ProjectSwitch {
        stage: ProjectSwitchStage::FlushDocuments,
        target: Project {
            id: ProjectId(uuid::Uuid::new_v4()),
            name: "Project B".into(),
            created_at: "2".into(),
        },
        operation: dexo_tui::runtime::OperationId::new(),
    });
    let _ = update(
        &mut model,
        Action::ProjectSwitchFailed {
            message: "disk full".into(),
        },
    );
    assert_eq!(model.project, "Project A");
    assert!(model.projects.pending.is_none());
}

#[test]
fn project_crud_rejects_duplicate_and_empty_names() {
    let db = Database::open_in_memory().unwrap();
    let repo = ProjectRepository::new(db.connection());
    repo.create("demo").unwrap();
    assert!(
        repo.create("demo")
            .unwrap_err()
            .to_string()
            .contains("exists")
    );
    assert!(
        repo.create("  ")
            .unwrap_err()
            .to_string()
            .contains("required")
    );
}

#[test]
fn project_crud_preview_detaches_and_keeps_external_paths() {
    let db = Database::open_in_memory().unwrap();
    let repo = ProjectRepository::new(db.connection());
    let project = repo.create("demo").unwrap();
    let pid = project.id.0.to_string();
    DocumentRepository::new(db.connection())
        .save(
            "d1",
            Some(&pid),
            "scratch",
            "select 1",
            Some("C:/tmp/keep.sql"),
            None,
        )
        .unwrap();
    let preview = repo.preview_delete(project.id).unwrap();
    assert_eq!(preview.documents, 1);
    assert_eq!(preview.external_paths, vec!["C:/tmp/keep.sql".to_string()]);
    repo.delete(project.id).unwrap();
    assert!(repo.get(project.id).unwrap().is_none());
}

#[test]
fn project_crud_recent_ordering_follows_touch() {
    let mut model = Model::default();
    model.projects.touch_recent("A");
    model.projects.touch_recent("B");
    model.projects.touch_recent("A");
    assert_eq!(
        model.projects.recents,
        vec!["A".to_string(), "B".to_string()]
    );
}

#[tokio::test]
async fn config_import_previews_conflicts_and_generates_fresh_secret_refs() {
    let existing = Database::open_in_memory().unwrap();
    ConnectionRepository::new(existing.connection())
        .save(&dexo_app::ConnectionProfile::new(
            dexo_app::ConnectionId(uuid::Uuid::new_v4()),
            None,
            "local-pg",
            "postgres",
            "local",
            serde_json::json!({"host":"localhost","port":5432}),
            dexo_app::SecretRef::new("old-ref".into()),
        ))
        .unwrap();
    let portable = Database::open_in_memory().unwrap();
    ConnectionRepository::new(portable.connection())
        .save(&dexo_app::ConnectionProfile::new(
            dexo_app::ConnectionId(uuid::Uuid::new_v4()),
            None,
            "local-pg",
            "postgres",
            "local",
            serde_json::json!({"host":"localhost","port":5432}),
            dexo_app::SecretRef::new("secret-123".into()),
        ))
        .unwrap();
    let toml = export_portable(portable.connection()).unwrap();
    let preview = preview_import(existing.connection(), &toml).unwrap();
    assert_eq!(preview.conflicts, vec!["local-pg"]);
    let mut resolutions = std::collections::HashMap::new();
    resolutions.insert(
        "local-pg".into(),
        ImportResolution::Rename("local-pg-2".into()),
    );
    let report = import_portable_resolved(existing.connection(), &toml, &resolutions).unwrap();
    assert_eq!(report.connections_needing_secret, vec!["local-pg-2"]);
    let dumped = format!(
        "{:?}",
        ConnectionRepository::new(existing.connection())
            .list()
            .unwrap()
    );
    assert!(!dumped.contains("secret-123"));
}
