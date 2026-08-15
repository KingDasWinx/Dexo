#[test]
fn deleting_connection_does_not_delete_secret() {
    let db = dexo_storage::Database::open_in_memory().unwrap();
    let repo = dexo_storage::ConnectionRepository::new(db.connection());
    let profile = dexo_app::ConnectionProfile::new(
        dexo_app::ConnectionId(uuid::Uuid::new_v4()),
        None,
        "local-pg",
        "postgres",
        "local",
        serde_json::json!({"host":"localhost","port":5432}),
        dexo_app::SecretRef::new("secret-123".into()),
    );
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

    let mut profile = dexo_app::ConnectionProfile::new(
        dexo_app::ConnectionId(uuid::Uuid::new_v4()),
        Some(project.id.0),
        "local-pg",
        "postgres",
        "local",
        serde_json::json!({"host":"localhost","password":"nope"}),
        dexo_app::SecretRef::new("secret-123".into()),
    );
    profile.group_path = Some("lab/pg".into());
    connections.save(&profile).unwrap();
    let loaded = connections.get(profile.id).unwrap().unwrap();
    assert_eq!(
        connections.get_by_name("local-pg").unwrap().unwrap().id,
        profile.id
    );
    assert_eq!(loaded.name, "local-pg");
    assert_eq!(loaded.secret_ref.as_str(), "secret-123");
    assert_eq!(
        loaded.secret_refs[dexo_app::PURPOSE_DATABASE_PASSWORD].as_str(),
        "secret-123"
    );
    assert_eq!(loaded.group_path.as_deref(), Some("lab/pg"));
    assert!(loaded.config.get("password").is_none());
}

#[test]
fn connection_crud_duplicate_group_and_project_list() {
    let db = dexo_storage::Database::open_in_memory().unwrap();
    let projects = dexo_storage::ProjectRepository::new(db.connection());
    let connections = dexo_storage::ConnectionRepository::new(db.connection());
    let project = dexo_app::Project {
        id: dexo_app::ProjectId(uuid::Uuid::new_v4()),
        name: "work".into(),
        created_at: "now".into(),
    };
    projects.save(&project).unwrap();
    let original = dexo_app::ConnectionProfile::new(
        dexo_app::ConnectionId(uuid::Uuid::new_v4()),
        Some(project.id.0),
        "src",
        "postgres",
        "local",
        serde_json::json!({"host":"localhost"}),
        dexo_app::SecretRef::new("secret-a".into()),
    );
    connections.save(&original).unwrap();

    let mut updated = original.clone();
    updated.environment = "staging".into();
    connections.update(&updated).unwrap();
    assert_eq!(
        connections.get(original.id).unwrap().unwrap().environment,
        "staging"
    );

    connections
        .move_group(original.id, Some("prod/east"))
        .unwrap();
    assert_eq!(
        connections
            .get(original.id)
            .unwrap()
            .unwrap()
            .group_path
            .as_deref(),
        Some("prod/east")
    );

    let copy = connections.duplicate(original.id).unwrap();
    assert_ne!(copy.id, original.id);
    assert_ne!(copy.secret_ref.as_str(), original.secret_ref.as_str());
    assert_eq!(copy.group_path.as_deref(), Some("prod/east"));
    assert!(connections.get(original.id).unwrap().is_some());

    let listed = connections.list_for_project(project.id.0).unwrap();
    assert_eq!(listed.len(), 2);
}
