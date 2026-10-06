//! `dexo plugin …` from the outside, every run in a scratch data folder.
use std::path::Path;

use assert_cmd::Command;
use dexo_test_support::plugins::{
    HttpServer, TestKey, index_json, pack, theme_folder, version_entry,
};
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

/// An index serving the theme pack `pack` at each `(version, min_dexo)`, signed.
struct Published {
    server: HttpServer,
    key: TestKey,
}

fn publish(home: &Path, versions: &[(&str, &str)]) -> Published {
    let server = HttpServer::start();
    let key = TestKey::new();
    let entries = versions
        .iter()
        .map(|(version, min_dexo)| {
            let bytes = pack(&theme_folder(home, "pack", version));
            let path = format!("/pack-{version}.tar.gz");
            server.put(&path, bytes.clone());
            version_entry(version, min_dexo, "any", &server.url(&path), &bytes, None)
        })
        .collect();
    let published = Published { server, key };
    published.sign(&index_json(&[("pack", "theme", entries)]));
    published
}

impl Published {
    fn sign(&self, index: &str) {
        self.server.put("/index.json", index);
        self.server
            .put("/index.json.minisig", self.key.sign(index.as_bytes()));
    }
}

fn dexo_with(home: &Path, published: &Published) -> Command {
    let mut command = dexo(home);
    command
        .env("DEXO_PLUGIN_INDEX", published.server.url("/index.json"))
        .env("DEXO_PLUGIN_TRUSTED_KEY", published.key.public());
    command
}

#[test]
fn install_from_the_index_then_update() {
    let home = tempfile::tempdir().unwrap();
    let published = publish(home.path(), &[("1.0.0", "1.0.0")]);
    dexo_with(home.path(), &published)
        .args(["plugin", "list", "--available"])
        .assert()
        .success()
        .stdout(contains("pack"));
    let available = dexo_with(home.path(), &published)
        .args(["plugin", "list", "--available", "--json"])
        .output()
        .unwrap();
    let available: serde_json::Value = serde_json::from_slice(&available.stdout).unwrap();
    assert_eq!(
        (
            available[0]["id"].as_str(),
            available[0]["version"].as_str()
        ),
        (Some("pack"), Some("1.0.0"))
    );
    dexo_with(home.path(), &published)
        .args(["plugin", "install", "pack"])
        .assert()
        .success()
        .stdout(contains("Installed pack 1.0.0 (official, signed)"));
    dexo_with(home.path(), &published)
        .args(["plugin", "list"])
        .assert()
        .stdout(contains("official"));
    let newer = publish(home.path(), &[("1.0.0", "1.0.0"), ("1.1.0", "1.0.0")]);
    dexo_with(home.path(), &newer)
        .args(["plugin", "update", "--all"])
        .assert()
        .success()
        .stdout(contains("Updated pack to 1.1.0"));
    dexo_with(home.path(), &newer)
        .args(["plugin", "update", "pack"])
        .assert()
        .success()
        .stdout(contains("pack is up to date"));
}

#[test]
fn a_download_unlike_the_index_installs_nothing() {
    let home = tempfile::tempdir().unwrap();
    let published = publish(home.path(), &[("1.0.0", "1.0.0")]);
    // Same length, other bytes: the checksum catches it. (The served bytes, not a new
    // pack: a package rebuilt carries new file times.)
    let real = published.server.bytes("/pack-1.0.0.tar.gz");
    let mut other = real.clone();
    let last = other.len() - 1;
    other[last] ^= 0xff;
    published.server.put("/pack-1.0.0.tar.gz", other);
    dexo_with(home.path(), &published)
        .args(["plugin", "install", "pack"])
        .assert()
        .failure()
        .stderr(contains(
            "does not match the index's checksum: nothing was installed",
        ));
    // Shorter than the index says: the size catches it.
    published
        .server
        .put("/pack-1.0.0.tar.gz", real[..real.len() - 10].to_vec());
    dexo_with(home.path(), &published)
        .args(["plugin", "install", "pack"])
        .assert()
        .failure()
        .stderr(contains("nothing was installed"));
    assert!(!home.path().join("data/plugins/pack").exists());
}

#[test]
fn what_this_dexo_cannot_run_or_trust_is_refused() {
    let home = tempfile::tempdir().unwrap();
    let published = publish(home.path(), &[("1.0.0", "99.0.0")]);
    dexo_with(home.path(), &published)
        .args(["plugin", "install", "pack"])
        .assert()
        .failure()
        .stderr(contains("needs Dexo 99.0.0 or newer"));
    let mut untrusted = dexo(home.path());
    untrusted
        .env("DEXO_PLUGIN_INDEX", published.server.url("/index.json"))
        .env("DEXO_PLUGIN_TRUSTED_KEY", TestKey::new().public())
        .args(["plugin", "install", "pack"])
        .assert()
        .failure()
        .stderr(contains(
            "the index signature does not match: nothing was installed",
        ));
}

#[test]
fn offline_the_cached_index_is_shown_with_its_date() {
    let home = tempfile::tempdir().unwrap();
    let published = publish(home.path(), &[("1.0.0", "1.0.0")]);
    dexo_with(home.path(), &published)
        .args(["plugin", "list", "--available"])
        .assert()
        .success();
    let mut offline = dexo(home.path());
    offline
        .env("DEXO_PLUGIN_INDEX", HttpServer::dead_url())
        .env("DEXO_PLUGIN_TRUSTED_KEY", published.key.public())
        .args(["plugin", "list", "--available"])
        .assert()
        .success()
        .stdout(contains("pack"))
        .stderr(contains("offline: showing the index from"));
}

#[test]
fn doctor_finds_a_changed_file_and_a_held_lock_refuses_an_install() {
    let home = tempfile::tempdir().unwrap();
    let published = publish(home.path(), &[("1.0.0", "1.0.0")]);
    dexo_with(home.path(), &published)
        .args(["plugin", "install", "pack"])
        .assert()
        .success();
    let report = dexo(home.path())
        .args(["plugin", "doctor", "--json"])
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report[0]["problems"], serde_json::json!([]));
    dexo(home.path())
        .args(["doctor"])
        .assert()
        .success()
        .stdout(contains("Plugins: 1 installed"));
    std::fs::write(
        home.path().join("data/plugins/pack/themes/ocean.toml"),
        "changed",
    )
    .unwrap();
    dexo(home.path())
        .args(["plugin", "doctor"])
        .assert()
        .failure()
        .stdout(contains(
            "themes/ocean.toml was changed since it was installed",
        ));
    let locks = home.path().join("data/plugins/.locks");
    std::fs::create_dir_all(&locks).unwrap();
    std::fs::write(locks.join("pack.lock"), "1").unwrap();
    dexo_with(home.path(), &published)
        .args(["plugin", "update", "pack"])
        .assert()
        .stdout(contains("pack is up to date"));
    dexo_with(home.path(), &published)
        .args(["plugin", "install", "pack", "--version", "1.0.0"])
        .assert()
        .failure()
        .stderr(contains("another Dexo is installing or removing plugins"));
}

#[test]
fn a_file_named_like_a_plugin_does_not_hide_it() {
    let home = tempfile::tempdir().unwrap();
    let published = publish(home.path(), &[("1.0.0", "1.0.0")]);
    let here = home.path().join("here");
    std::fs::create_dir_all(&here).unwrap();
    std::fs::write(here.join("pack"), "not a package").unwrap();
    dexo_with(home.path(), &published)
        .current_dir(&here)
        .args(["plugin", "install", "pack"])
        .assert()
        .success()
        .stdout(contains("Installed pack 1.0.0 (official, signed)"));
}
