use dexo_storage::{ConnectionRepository, Database, apply_pending};

#[test]
fn fresh_database_reaches_schema_four() {
    let db = Database::open_in_memory().unwrap();
    assert_eq!(db.schema_version().unwrap(), 8);
}

#[test]
fn foreign_keys_are_enabled() {
    let db = Database::open_in_memory().unwrap();
    let enabled: i64 = db
        .connection()
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(enabled, 1);
}

#[test]
fn connections_table_has_secret_ref_not_password() {
    let db = Database::open_in_memory().unwrap();
    let mut stmt = db
        .connection()
        .prepare("PRAGMA table_info(connections)")
        .unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(|c| c.unwrap())
        .collect();
    assert!(columns.iter().any(|c| c == "secret_ref"));
    assert!(columns.iter().any(|c| c == "group_path"));
    assert!(columns.iter().any(|c| c == "policy_json"));
    assert!(!columns.iter().any(|c| c == "password" || c == "secret"));
}

fn database_at_version(version: u32) -> Database {
    let db = Database::open_in_memory_at(version).unwrap();
    if version >= 1 {
        let project = uuid::Uuid::nil();
        let connection = uuid::Uuid::from_u128(1);
        db.connection()
            .execute(
                "INSERT INTO projects(id, name, created_at) VALUES (?1, 'Default', datetime('now'))",
                rusqlite::params![project.to_string()],
            )
            .unwrap();
        db.connection()
            .execute(
                "INSERT INTO connections(id, project_id, name, driver, environment, config_json, secret_ref)
                 VALUES (?1, ?2, 'legacy', 'postgres', 'local', '{}', 'legacy-ref')",
                rusqlite::params![connection.to_string(), project.to_string()],
            )
            .unwrap();
    }
    db
}

#[test]
fn migration_8_moves_the_legacy_password_ref_and_preserves_profiles() {
    let db = database_at_version(7);
    apply_pending(db.connection()).unwrap();
    let purposes: i64 = db
        .connection()
        .query_row(
            "select count(*) from connection_secret_refs where purpose='database_password'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(purposes, 1);
    assert!(
        ConnectionRepository::new(db.connection())
            .get_by_name("legacy")
            .unwrap()
            .is_some()
    );
}
