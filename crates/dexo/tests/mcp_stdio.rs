use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use dexo_app::mcp::{Effect, McpProfile, SelectorRule};
use dexo_storage::{AppPaths, Database, McpProfileRepository};

#[test]
fn serve_stdout_is_jsonrpc_with_debug_logging() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_home(dir.path().to_path_buf());
    let db = Database::open(&paths.database).unwrap();
    let mut profile = McpProfile::new("conformance-fixture");
    profile.enabled = true;
    profile.selectors = vec![SelectorRule::parse(Effect::Allow, "db.public.*").unwrap()];
    McpProfileRepository::new(db.connection())
        .save(&profile)
        .unwrap();
    drop(db);

    let bin = assert_cmd::cargo::cargo_bin("dexo");
    let mut child = Command::new(bin)
        .env("DEXO_DATA_HOME", dir.path())
        .env("RUST_LOG", "debug")
        .args(["mcp", "serve", "--profile", "conformance-fixture"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0.0.1"}}}"#;
    let initialized = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    stdin.write_all(init.as_bytes()).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.write_all(initialized.as_bytes()).unwrap();
    stdin.write_all(b"\n").unwrap();
    drop(stdin);
    let output = wait_output(child, Duration::from_secs(8));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(
            parsed.is_ok(),
            "stdout line is not JSON-RPC: {line}\nstderr={stderr}"
        );
        assert_eq!(parsed.unwrap()["jsonrpc"], "2.0");
    }
    assert!(
        !stderr.contains("\"jsonrpc\""),
        "protocol response leaked to stderr: {stderr}"
    );
}

fn wait_output(mut child: std::process::Child, timeout: Duration) -> std::process::Output {
    let start = std::time::Instant::now();
    loop {
        if let Some(_status) = child.try_wait().unwrap() {
            return child.wait_with_output().unwrap();
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            return child.wait_with_output().unwrap();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
