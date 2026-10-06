//! `dexo plugin …` from the outside, every run in a scratch data folder.
use std::path::Path;

use assert_cmd::Command;
use dexo_test_support::plugins::{pack, theme_folder};
use predicates::str::contains;

fn dexo(home: &Path) -> Command {
    let mut command = Command::cargo_bin("dexo").unwrap();
    command
        .env("DEXO_DATA_HOME", home.join("data"))
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("DEXO_NO_UPDATE_CHECK", "1");
    command
}

#[test]
fn a_local_package_installs_only_with_trust() {
    let home = tempfile::tempdir().unwrap();
    let folder = theme_folder(home.path(), "pack", "1.0.0");
    let file = home.path().join("pack.tar.gz");
    std::fs::write(&file, pack(&folder)).unwrap();
    dexo(home.path())
        .args(["plugin", "install"])
        .arg(&file)
        .assert()
        .failure()
        .stderr(contains("is not from Dexo's signed index"))
        .stderr(contains("--trust"));
    dexo(home.path())
        .args(["plugin", "install", "--trust"])
        .arg(&file)
        .assert()
        .success()
        .stdout(contains("Installed pack 1.0.0 (local, not verified)"));
    dexo(home.path())
        .args(["plugin", "list"])
        .assert()
        .success()
        .stdout(contains("pack"))
        .stdout(contains("enabled"))
        .stdout(contains("local"));
    let listed = dexo(home.path())
        .args(["plugin", "list", "--json"])
        .output()
        .unwrap();
    let rows: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(rows[0]["id"], "pack");
    assert_eq!(rows[0]["origin"], "local");
    let info = dexo(home.path())
        .args(["plugin", "info", "pack", "--json"])
        .output()
        .unwrap();
    let info: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    assert_eq!(info["provides"], serde_json::json!(["ocean"]));
}

#[test]
fn disable_enable_and_remove() {
    let home = tempfile::tempdir().unwrap();
    let folder = theme_folder(home.path(), "pack", "1.0.0");
    dexo(home.path())
        .args(["plugin", "install", "--trust"])
        .arg(&folder)
        .assert()
        .success();
    dexo(home.path())
        .args(["plugin", "disable", "pack"])
        .assert()
        .success();
    dexo(home.path())
        .args(["plugin", "list"])
        .assert()
        .stdout(contains("disabled"));
    dexo(home.path())
        .args(["plugin", "enable", "pack"])
        .assert()
        .success();
    dexo(home.path())
        .args(["plugin", "remove", "pack"])
        .assert()
        .failure()
        .stderr(contains("--confirm"));
    dexo(home.path())
        .args(["plugin", "remove", "pack", "--confirm"])
        .assert()
        .success();
    dexo(home.path())
        .args(["plugin", "list"])
        .assert()
        .stdout(contains("No plugins are installed."));
}
