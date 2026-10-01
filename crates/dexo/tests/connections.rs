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

/// A SQLite connection is a path: `connections add` asks for no host, user or password,
/// and `query` opens the file without the keychain.
#[test]
fn a_sqlite_connection_is_a_path_that_answers_queries() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("shop.db");
    let dexo = || {
        let mut command = Command::cargo_bin("dexo").unwrap();
        command.env("DEXO_DATA_HOME", dir.path());
        command
    };
    dexo()
        .args(["connections", "add", "--name", "shop", "--driver", "sqlite"])
        .args(["--path", file.to_str().unwrap(), "--no-test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("saved shop"));
    dexo()
        .args(["query", "--connection", "shop", "--sql"])
        .arg("create table t (n integer); insert into t values (7)")
        .assert()
        .success();
    dexo()
        .args(["query", "--connection", "shop", "--sql", "select n from t"])
        .args(["--format", "jsonl", "--non-interactive"])
        .assert()
        .success()
        .stdout("{\"n\":7}\n");
}
