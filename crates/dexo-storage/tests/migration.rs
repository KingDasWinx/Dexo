use dexo_storage::Database;

#[test]
fn fresh_database_reaches_schema_four() {
    let db = Database::open_in_memory().unwrap();
    assert_eq!(db.schema_version().unwrap(), 7);
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
    assert!(!columns.iter().any(|c| c == "password" || c == "secret"));
}
