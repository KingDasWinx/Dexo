use std::env;
use std::path::Path;
use std::process::Command;

use dexo_app::diagnostic_service::SECRET_SENTINEL;
use dexo_app::recovery_service::{RecoveryDocumentDraft, sanitize_checkpoint};
use dexo_app::{Project, ProjectId};
use dexo_storage::{
    Database, LayoutRepository, ProjectRepository, RecoveryRepository, SessionRecoveryRepository,
    WorkbenchLayout,
};

#[test]
fn kill_reopen_offers_recovery_without_secrets() {
    if let Ok(path) = env::var("DEXO_RECOVERY_DB") {
        crash_child(Path::new(&path));
        std::process::exit(99);
    }
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("dexo.db");
    let status = Command::new(env::current_exe().unwrap())
        .args([
            "kill_reopen_offers_recovery_without_secrets",
            "--exact",
            "--nocapture",
        ])
        .env("DEXO_RECOVERY_DB", &db_path)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(99));
    let db = Database::open(&db_path).unwrap();
    let projects = ProjectRepository::new(db.connection()).list().unwrap();
    let project = projects[0].id.0.to_string();
    let state = SessionRecoveryRepository::new(db.connection())
        .load(&project)
        .unwrap();
    assert!(state.needs_recovery());
    assert_eq!(state.transaction, "unknown");
    let bytes = std::fs::read(&db_path).unwrap();
    assert!(
        !bytes
            .windows(SECRET_SENTINEL.len())
            .any(|window| window == SECRET_SENTINEL.as_bytes())
    );
}

fn crash_child(path: &Path) {
    let db = Database::open(path).unwrap();
    let id = uuid::Uuid::new_v4();
    ProjectRepository::new(db.connection())
        .save(&Project {
            id: ProjectId(id),
            name: "p".into(),
            created_at: "now".into(),
        })
        .unwrap();
    let project = id.to_string();
    let checkpoint = sanitize_checkpoint(
        vec![RecoveryDocumentDraft {
            id: "doc-1".into(),
            title: "scratch".into(),
            content: format!("select '{SECRET_SENTINEL}'"),
        }],
        Some(format!("{{\"password\":\"{SECRET_SENTINEL}\"}}")),
        true,
        &[SECRET_SENTINEL],
    );
    RecoveryRepository::new(db.connection())
        .checkpoint(
            &checkpoint.documents[0].id,
            &project,
            &checkpoint.documents[0].title,
            &checkpoint.documents[0].content,
        )
        .unwrap();
    LayoutRepository::new(db.connection())
        .save(&project, &WorkbenchLayout::default())
        .unwrap();
    SessionRecoveryRepository::new(db.connection())
        .checkpoint_layout(&WorkbenchLayout::default(), "active")
        .unwrap();
    drop(db);
}
