//! The release's index script writes an index Dexo reads, listing a package as it is,
//! and refuses a version already listed. Linux only: the script needs Python 3.11.
#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::Command;

use dexo_test_support::plugins::{manifest, pack, theme_folder};

fn run(current: &Path, manifest: &Path, dist: &Path) -> std::process::Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/plugin-index.py");
    Command::new("python3")
        .arg(script)
        .arg(current)
        .arg(manifest)
        .arg(dist)
        .arg("https://example.invalid/r")
        .output()
        .expect("python3")
}

#[test]
fn the_release_script_writes_an_index_dexo_reads() {
    let work = tempfile::tempdir().unwrap();
    let dist = work.path().join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    let folder = theme_folder(work.path(), "pack", "1.0.0");
    let bytes = pack(&folder);
    std::fs::write(dist.join("pack-1.0.0.tar.gz"), &bytes).unwrap();
    let manifest = folder.join("dexo-plugin.toml");
    let first = run(&work.path().join("missing.json"), &manifest, &dist);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let index: dexo_app::plugin::index::Index = serde_json::from_slice(&first.stdout).unwrap();
    let artifact = &index.plugins[0].versions[0].artifacts["any"];
    assert_eq!(artifact.size, bytes.len() as u64);
    assert_eq!(artifact.url, "https://example.invalid/r/pack-1.0.0.tar.gz");
    let current = work.path().join("index.json");
    std::fs::write(&current, &first.stdout).unwrap();
    let again = run(&current, &manifest, &dist);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already in the index"));
}

#[test]
fn the_release_script_refuses_a_driver_dexo_or_another_plugin_has() {
    let work = tempfile::tempdir().unwrap();
    let dist = work.path().join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    let index = r#"{"schema": 1, "generated": "", "plugins": [{"id": "lake", "kind": "driver", "name": "Lake", "versions": [{"version": "1.0.0", "min_dexo": "1.0.0", "driver": {"protocol": 1, "drivers": ["lake"], "display_name": "Lake"}, "artifacts": {}}]}]}"#;
    let current = work.path().join("index.json");
    std::fs::write(&current, index).unwrap();
    for (id, driver) in [("evil", "postgres"), ("evil", "duckdb"), ("evil", "lake")] {
        std::fs::write(
            dist.join(format!("{id}-1.0.0-x86_64-unknown-linux-gnu.tar.gz")),
            b"x",
        )
        .unwrap();
        let path = work.path().join("dexo-plugin.toml");
        std::fs::write(
            &path,
            manifest(id, "1.0.0", "1.0.0", Some("evil")).replace(
                &format!("drivers = [\"{id}\"]"),
                &format!("drivers = [\"{driver}\"]"),
            ),
        )
        .unwrap();
        let out = run(&current, &path, &dist);
        assert!(!out.status.success(), "{driver}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(&format!("'{driver}' is a driver")),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
