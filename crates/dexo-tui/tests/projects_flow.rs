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
            .any(|message| message.message.contains("transaction"))
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
            None,
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

fn stored(
    id: &str,
    title: &str,
    content: &str,
    path: Option<&str>,
) -> dexo_storage::StoredDocument {
    dexo_storage::StoredDocument {
        id: id.into(),
        project_id: Some("p".into()),
        title: title.into(),
        content: content.into(),
        path: path.map(str::to_string),
        fingerprint: None,
        kind: None,
        connection_id: Some(uuid::Uuid::from_u128(7).to_string()),
    }
}

/// A document left behind by a project switch comes back as the tab it was: named, bound
/// to its connection and, being text no file holds, unsaved. It used to come back titled
/// with its internal id, and as if it had been saved.
#[test]
fn a_project_loaded_brings_its_documents_back_by_name_and_unsaved() {
    let mut model = Model::default();
    let _ = update(
        &mut model,
        Action::ProjectLoaded {
            project: Project {
                id: ProjectId(uuid::Uuid::new_v4()),
                name: "Project B".into(),
                created_at: "2".into(),
            },
            documents: vec![
                stored("e7c03b76-95a8", "doc1.sql", "select 1", None),
                stored("0d1c", "empty.sql", "", None),
            ],
            layout: None,
            recent_sql_files: Vec::new(),
        },
    );
    let draft = &model.documents[0];
    assert_eq!(draft.title, "doc1.sql");
    assert_eq!(
        draft.connection_id.as_deref(),
        Some(uuid::Uuid::from_u128(7).to_string().as_str())
    );
    assert!(draft.is_dirty(), "a draft with no file is unsaved");
    assert!(!model.documents[1].is_dirty(), "an empty one is not");
}

/// A document with a file is unsaved only when the file says something else.
#[test]
fn a_restored_document_with_a_file_is_unsaved_only_when_the_file_differs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("q.sql");
    std::fs::write(&path, "select 1").unwrap();
    let path = path.to_string_lossy().into_owned();
    let same = dexo_tui::update::document_from_stored_for_test(stored(
        "a",
        "q.sql",
        "select 1",
        Some(&path),
    ));
    assert!(!same.is_dirty());
    let edited = dexo_tui::update::document_from_stored_for_test(stored(
        "b",
        "q.sql",
        "select 2",
        Some(&path),
    ));
    assert!(edited.is_dirty());
}

fn two_projects() -> (Model, Project, Project) {
    let (a, b) = (
        Project {
            id: ProjectId(uuid::Uuid::from_u128(1)),
            name: "Default".into(),
            created_at: "1".into(),
        },
        Project {
            id: ProjectId(uuid::Uuid::from_u128(2)),
            name: "qa".into(),
            created_at: "2".into(),
        },
    );
    let mut model = Model {
        project: "Default".into(),
        project_id: a.id.0.to_string(),
        ..Model::default()
    };
    model.apply_size(100, 30);
    model.projects.load(vec![a.clone(), b.clone()]);
    (model, a, b)
}

fn key(code: crossterm::event::KeyCode) -> Action {
    Action::Key(crossterm::event::KeyEvent::new(
        code,
        crossterm::event::KeyModifiers::NONE,
    ))
}

/// Leaving a project with unsaved documents asks, with the three answers of closing a tab.
/// It showed `switch to qa (ConfirmDirty)` and waited for a letter it did not name.
#[test]
fn leaving_a_project_with_unsaved_documents_asks_save_dont_save_or_cancel() {
    use crossterm::event::KeyCode;
    let (mut model, _, qa) = two_projects();
    model
        .active_document_mut()
        .sql
        .insert(0, "select 1")
        .unwrap();
    model.projects.open = true;
    let effects = update(&mut model, Action::ProjectSwitchTarget(qa.clone()));
    assert!(
        effects.is_empty(),
        "nothing moves before the answer: {effects:?}"
    );
    let screen = dexo_tui::render::render_to_string(&model, 100, 30);
    for part in ["Unsaved changes", "[Save]", "[Don't save]", "[Cancel]"] {
        assert!(screen.contains(part), "{part}:\n{screen}");
    }
    assert!(!screen.contains("ConfirmDirty"), "{screen}");

    // Esc is Cancel: the switch is dropped and the document is still there.
    update(&mut model, key(KeyCode::Esc));
    assert!(model.projects.pending.is_none());
    assert!(model.active_document().is_dirty());

    // Don't save closes the unsaved document and goes on.
    update(&mut model, Action::ProjectSwitchTarget(qa.clone()));
    let effects = update(&mut model, key(KeyCode::Char('d')));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::FlushDocuments { .. })),
        "{effects:?}"
    );
    assert!(model.documents.iter().all(|document| !document.is_dirty()));

    // Save keeps them in the project and goes on too.
    let (mut model, _, qa) = two_projects();
    model
        .active_document_mut()
        .sql
        .insert(0, "select 2")
        .unwrap();
    model.projects.open = true;
    update(&mut model, Action::ProjectSwitchTarget(qa));
    let effects = update(&mut model, key(KeyCode::Enter));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::FlushDocuments { .. })),
        "{effects:?}"
    );
}

/// Deleting the project that is open moves to another, saving nothing into the project
/// that is gone. It left the app without a project, its document on screen, and every
/// switch after it failed on a foreign key.
#[test]
fn deleting_the_open_project_moves_to_another_without_saving_into_it() {
    let (mut model, default, qa) = two_projects();
    model.project = "qa".into();
    model.project_id = qa.id.0.to_string();
    model
        .active_document_mut()
        .sql
        .insert(0, "select 1")
        .unwrap();
    let effects = update(&mut model, Action::ProjectDeleted { name: "qa".into() });
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CloseProjectSessions)),
        "{effects:?}"
    );
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            Effect::FlushDocuments { .. } | Effect::PersistLayout { .. }
        )),
        "nothing is saved into the deleted project: {effects:?}"
    );
    assert_eq!(
        model
            .projects
            .pending
            .as_ref()
            .map(|switch| switch.target.id),
        Some(default.id)
    );
    assert!(
        model
            .documents
            .iter()
            .all(|document| document.kind.is_placeholder())
    );
}

/// A rename of the open project is the open project's name from then on: the header kept
/// the old one.
#[test]
fn renaming_the_open_project_renames_the_header() {
    let (mut model, default, qa) = two_projects();
    let renamed = Project {
        name: "Work".into(),
        ..default
    };
    update(&mut model, Action::ProjectsLoaded(vec![renamed, qa]));
    assert_eq!(model.project, "Work");
    assert!(
        !model.projects.recents.contains(&"Default".to_string()),
        "the old name is not a recent project"
    );
}

/// One project is not deleted: Dexo keeps at least one, and says so, where it used to ask
/// for a name and then fail.
#[test]
fn the_last_project_is_not_offered_for_deletion() {
    let (mut model, default, _) = two_projects();
    model.projects.load(vec![default]);
    let effects = update(&mut model, Action::DeleteProject);
    assert!(effects.is_empty());
    assert!(
        model
            .projects
            .error
            .as_deref()
            .unwrap_or("")
            .contains("at least one")
    );
}
