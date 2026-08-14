#[test]
fn deleting_connection_does_not_delete_secret() {
    let db = dexo_storage::Database::open_in_memory().unwrap();
    let repo = dexo_storage::ConnectionRepository::new(db.connection());
    let profile = dexo_app::ConnectionProfile {
        id: dexo_app::ConnectionId(uuid::Uuid::new_v4()),
        project_id: None,
        name: "local-pg".into(),
        driver: "postgres".into(),
        environment: "local".into(),
        config: serde_json::json!({"host":"localhost","port":5432}),
        secret_ref: dexo_app::SecretRef::new("secret-123".into()),
    };
    repo.save(&profile).unwrap();
    repo.delete(profile.id).unwrap();
    assert!(repo.get(profile.id).unwrap().is_none());
    assert_eq!(profile.secret_ref.as_str(), "secret-123");
}

#[test]
fn project_and_connection_round_trip() {
    let db = dexo_storage::Database::open_in_memory().unwrap();
    let projects = dexo_storage::ProjectRepository::new(db.connection());
    let connections = dexo_storage::ConnectionRepository::new(db.connection());
    let project = dexo_app::Project {
        id: dexo_app::ProjectId(uuid::Uuid::new_v4()),
        name: "work".into(),
        created_at: "2026-08-14T00:00:00Z".into(),
    };
    projects.save(&project).unwrap();
    assert_eq!(projects.get(project.id).unwrap().unwrap(), project);

    let profile = dexo_app::ConnectionProfile {
        id: dexo_app::ConnectionId(uuid::Uuid::new_v4()),
        project_id: Some(project.id.0),
        name: "local-pg".into(),
        driver: "postgres".into(),
        environment: "local".into(),
        config: serde_json::json!({"host":"localhost","password":"nope"}),
        secret_ref: dexo_app::SecretRef::new("secret-123".into()),
    };
    connections.save(&profile).unwrap();
    let loaded = connections.get(profile.id).unwrap().unwrap();
    assert_eq!(
        connections.get_by_name("local-pg").unwrap().unwrap().id,
        profile.id
    );
    assert_eq!(loaded.name, "local-pg");
    assert_eq!(loaded.secret_ref.as_str(), "secret-123");
    assert!(loaded.config.get("password").is_none());
}
