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
