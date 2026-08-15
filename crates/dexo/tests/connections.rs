use assert_cmd::Command;
use dexo_storage::{AppPaths, ConnectionRepository, Database};
use predicates::prelude::*;

#[test]
fn connections_add_lists_without_leaking_password() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("dexo")
        .unwrap()
        .env("DEXO_DATA_HOME", dir.path())
        .args([
            "connections",
            "add",
            "--name",
            "local-pg",
            "--driver",
            "postgres",
            "--host",
            "127.0.0.1",
            "--username",
            "dexo",
            "--database",
            "dexo",
            "--non-interactive",
            "--password-stdin",
            "--no-test",
        ])
        .write_stdin("SUPER_SECRET_SENTINEL\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("saved local-pg"))
        .stdout(predicate::str::contains("SUPER_SECRET_SENTINEL").not());

    let list = Command::cargo_bin("dexo")
        .unwrap()
        .env("DEXO_DATA_HOME", dir.path())
        .args(["connections", "list"])
        .assert()
        .success();
    list.stdout(predicate::str::contains("local-pg"))
        .stdout(predicate::str::contains("SUPER_SECRET_SENTINEL").not());

    let paths = AppPaths::from_data_home(dir.path().to_path_buf());
    let db = Database::open(&paths.database).unwrap();
    let loaded = ConnectionRepository::new(db.connection())
        .get_by_name("local-pg")
        .unwrap()
        .unwrap();
    assert!(!loaded.config.to_string().contains("SUPER_SECRET_SENTINEL"));
    assert!(!format!("{loaded:?}").contains("SUPER_SECRET_SENTINEL"));
}
