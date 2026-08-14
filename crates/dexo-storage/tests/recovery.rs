#[test]
fn latest_checkpoint_replaces_older_content() {
    let db = dexo_storage::Database::open_in_memory().unwrap();
    db.connection()
        .execute(
            "INSERT INTO projects(id, name, created_at) VALUES ('project-1', 'p', datetime('now'))",
            [],
        )
        .unwrap();
    let repo = dexo_storage::RecoveryRepository::new(db.connection());
    repo.checkpoint("doc-1", "project-1", "scratch", "select 1")
        .unwrap();
    repo.checkpoint("doc-1", "project-1", "scratch", "select 2")
        .unwrap();
    assert_eq!(repo.load("doc-1").unwrap().unwrap().content, "select 2");
}

#[test]
fn recovery_round_trip_and_clear() {
    let db = dexo_storage::Database::open_in_memory().unwrap();
    let project_id = uuid::Uuid::new_v4();
    dexo_storage::ProjectRepository::new(db.connection())
        .save(&dexo_app::Project {
            id: dexo_app::ProjectId(project_id),
            name: "p".into(),
            created_at: "2026-08-14T00:00:00Z".into(),
        })
        .unwrap();
    let repo = dexo_storage::RecoveryRepository::new(db.connection());
    let pid = project_id.to_string();
    repo.checkpoint("doc-1", &pid, "scratch", "select 1")
        .unwrap();
    assert_eq!(repo.list_for_project(&pid).unwrap().len(), 1);
    repo.clear("doc-1").unwrap();
    assert!(repo.load("doc-1").unwrap().is_none());
}
