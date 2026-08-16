use std::fs;
use std::path::PathBuf;

use dexo_app::{ConnectionId, ConnectionProfile, Project, ProjectId, SecretRef};
use dexo_storage::{
    ConnectionRepository, Database, MIGRATION_1, MIGRATION_2, MIGRATION_3, MIGRATION_4,
    MIGRATION_5, MIGRATION_6, MIGRATION_7, MIGRATION_8, MIGRATION_9, MIGRATION_10,
    ProjectRepository, export_portable, import_portable,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn stored_schema_fixtures_match_migrations() {
    let expected = [
        ("schema-v1.sql", MIGRATION_1),
        ("schema-v2.sql", MIGRATION_2),
        ("schema-v3.sql", MIGRATION_3),
        ("schema-v4.sql", MIGRATION_4),
        ("schema-v5.sql", MIGRATION_5),
        ("schema-v6.sql", MIGRATION_6),
        ("schema-v7.sql", MIGRATION_7),
        ("schema-v8.sql", MIGRATION_8),
        ("schema-v9.sql", MIGRATION_9),
        ("schema-v10.sql", MIGRATION_10),
    ];
    for (name, sql) in expected {
        let on_disk = fs::read_to_string(fixture(name)).unwrap();
        assert!(on_disk.contains("-- sanitized"));
        assert!(on_disk.contains(sql.trim()));
    }
    let config = fs::read_to_string(fixture("config-v1.toml")).unwrap();
    let prefs = dexo_storage::Preferences::from_toml(&config).unwrap();
    assert_eq!(prefs.theme, "dark");
}

#[test]
fn export_import_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dexo.db");
    let exported;
    {
        let db = Database::open(&path).unwrap();
        let pid = uuid::Uuid::new_v4();
        ProjectRepository::new(db.connection())
            .save(&Project {
                id: ProjectId(pid),
                name: "p".into(),
                created_at: "now".into(),
            })
            .unwrap();
        ConnectionRepository::new(db.connection())
            .save(&ConnectionProfile::new(
                ConnectionId(uuid::Uuid::new_v4()),
                Some(pid),
                "c",
                "postgres",
                "local",
                serde_json::json!({"host": "localhost", "port": 5432}),
                SecretRef::new("ref-1".into()),
            ))
            .unwrap();
        exported = export_portable(db.connection()).unwrap();
    }
    let db = Database::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 11);
    let fresh = Database::open_in_memory().unwrap();
    let report = import_portable(fresh.connection(), &exported).unwrap();
    assert_eq!(report.connections_needing_secret, vec!["c"]);
}
