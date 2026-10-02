use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn version_uses_stdout() {
    Command::cargo_bin("dexo")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with(concat!(
            "dexo ",
            env!("CARGO_PKG_VERSION")
        )));
}

#[test]
fn doctor_is_non_interactive() {
    Command::cargo_bin("dexo")
        .unwrap()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""status":"ok""#));
}

/// `mcp doctor --probe --json` prints one JSON document and nothing else; a Codex file
/// that does not parse says so, and a bare command is found on PATH.
#[test]
fn mcp_probe_json_is_only_json() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::create_dir_all(home.join(".cursor")).unwrap();
    std::fs::write(home.join(".codex/config.toml"), "[mcp_servers.dexo\n").unwrap();
    std::fs::write(
        home.join(".cursor/mcp.json"),
        r#"{"mcpServers": {"dexo": {"command": "sh"}}}"#,
    )
    .unwrap();
    let output = Command::cargo_bin("dexo")
        .unwrap()
        .current_dir(dir.path())
        .env("DEXO_DATA_HOME", dir.path().join("data"))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .args(["mcp", "doctor", "--probe", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let status = |client: &str| {
        report["probe"]["clients"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["client"] == client)
            .and_then(|entry| entry["status"].as_str())
            .unwrap()
            .to_string()
    };
    assert_eq!(status("codex"), "unparseable", "{report}");
    assert_eq!(status("claude-code"), "no_file", "{report}");
    if cfg!(unix) {
        assert_eq!(status("cursor"), "ok", "{report}");
    }
}

/// `inspect --refresh` alone caches the connection's catalog -- what `dexo lsp` reads --
/// and says how much it cached, rather than asking for another flag after caching it.
#[test]
fn inspect_refresh_alone_caches_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    // Any SQLite file with tables in it will do.
    let file = dir.path().join("shop.db");
    dexo_storage::Database::open(&file).unwrap();
    let dexo = || {
        let mut command = Command::cargo_bin("dexo").unwrap();
        command
            .env("DEXO_DATA_HOME", dir.path().join("data"))
            .env("HOME", dir.path());
        command
    };
    dexo()
        .args([
            "connections",
            "add",
            "--name",
            "lite",
            "--driver",
            "sqlite",
            "--path",
        ])
        .arg(&file)
        .assert()
        .success();
    let output = dexo()
        .args(["inspect", "--connection", "lite", "--refresh"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["cached"].as_u64().unwrap() > 1, "{report}");
}

/// `query`, `run`, `export`, `import` and `explain --analyze` hold every statement to
/// the connection's policy, as the editor does: a destructive one waits for --confirm,
/// production for the connection's name, and a read-only connection refuses writes. A
/// comment in front of a statement hides nothing.
#[test]
fn the_command_line_holds_sql_to_the_connection_policy() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let dexo = || {
        let mut command = Command::cargo_bin("dexo").unwrap();
        command.env("DEXO_DATA_HOME", &data).env("HOME", dir.path());
        command
    };
    for (name, environment) in [("lite", "local"), ("live", "production")] {
        dexo()
            .args(["connections", "add", "--name", name, "--driver", "sqlite"])
            .args(["--environment", environment, "--path"])
            .arg(dir.path().join(format!("{name}.db")))
            .assert()
            .success();
    }
    let query = |connection: &str, sql: &str, extra: &[&str]| {
        dexo()
            .args(["query", "--connection", connection, "--sql", sql])
            .args(["--non-interactive", "--format", "jsonl"])
            .args(extra)
            .output()
            .unwrap()
    };
    let refused = |output: std::process::Output, says: &str| {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            !output.status.success() && stderr.contains(says),
            "{stderr}"
        );
    };
    let count = |connection: &str| {
        String::from_utf8(query(connection, "select count(*) as n from t", &[]).stdout).unwrap()
    };

    assert!(
        query(
            "lite",
            "create table t (n int); insert into t values (1)",
            &[]
        )
        .status
        .success()
    );
    refused(
        query("lite", "/* x */ delete from t", &[]),
        "Pass --confirm",
    );
    assert_eq!(count("lite"), "{\"n\":1}\n");
    assert!(
        query("lite", "delete from t", &["--confirm"])
            .status
            .success()
    );
    assert_eq!(count("lite"), "{\"n\":0}\n");

    refused(
        query("live", "create table t (n int)", &[]),
        "--confirm-target live",
    );
    refused(
        query("live", "create table t (n int)", &["--confirm"]),
        "--confirm-target live",
    );
    refused(
        query(
            "live",
            "create table t (n int)",
            &["--confirm-target", "lite"],
        ),
        "does not match",
    );
    assert!(
        query(
            "live",
            "create table t (n int)",
            &["--confirm-target", "live"]
        )
        .status
        .success()
    );
    refused(
        dexo()
            .args(["run", "--connection", "live"])
            .write_stdin("insert into t values (2)")
            .output()
            .unwrap(),
        "--confirm-target live",
    );
    assert_eq!(count("live"), "{\"n\":0}\n");

    // A read-only connection, on the file `lite` wrote.
    let paths = dexo_storage::AppPaths::from_data_home(data.clone());
    let db = dexo_storage::Database::open(&paths.database).unwrap();
    let mut profile = dexo_app::ConnectionProfile::new(
        dexo_app::ConnectionId(uuid::Uuid::new_v4()),
        None,
        "ro",
        "sqlite",
        "local",
        serde_json::json!({ "path": dir.path().join("lite.db") }),
        dexo_app::SecretRef::new(uuid::Uuid::new_v4().to_string()),
    );
    profile.policy.read_only = Some(true);
    dexo_storage::ConnectionRepository::new(db.connection())
        .save(&profile)
        .unwrap();
    assert_eq!(count("ro"), "{\"n\":0}\n");
    refused(
        query("ro", "select 1; insert into t values (3)", &["--confirm"]),
        "ro is read-only, and statement 2 is not a read",
    );
    let csv = dir.path().join("out.csv");
    refused(
        dexo()
            .args([
                "export",
                "--connection",
                "lite",
                "--sql",
                "delete from t",
                "--output",
            ])
            .arg(&csv)
            .output()
            .unwrap(),
        "an export runs only reads",
    );
    std::fs::write(&csv, "n\n4\n").unwrap();
    refused(
        dexo()
            .args(["import", "--connection", "ro", "--table", "t", "--file"])
            .arg(&csv)
            .output()
            .unwrap(),
        "ro is read-only",
    );
    refused(
        dexo()
            .args(["explain", "--connection", "ro", "--sql", "delete from t"])
            .args(["--analyze", "--confirm"])
            .output()
            .unwrap(),
        "ro is read-only",
    );
    assert_eq!(count("lite"), "{\"n\":0}\n");
}
