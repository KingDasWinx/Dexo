//! Installing from a file or a folder: put in place whole or not at all, recorded in
//! `state.json`, and guarded by a lock against a second Dexo.
use std::time::{Duration, SystemTime};

use dexo_app::plugin::install;
use dexo_app::plugin::state::{self, Lock, Origin};
use dexo_test_support::plugins::{pack, theme_folder};

#[test]
fn a_theme_pack_installs_from_a_folder_and_from_a_package() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let folder = theme_folder(work.path(), "pack", "1.0.0");
    let staged = install::stage(data.path(), &folder).unwrap();
    assert_eq!(staged.manifest.id, "pack");
    install::commit(data.path(), staged, Origin::Local, None).unwrap();
    let plugins = data.path().join("plugins");
    assert!(plugins.join("pack/themes/ocean.toml").is_file());
    let installed = &state::load(&plugins).plugins["pack"];
    assert_eq!(
        (
            installed.version.as_str(),
            installed.origin,
            installed.enabled
        ),
        ("1.0.0", Origin::Local, true)
    );
    assert!(installed.files.contains_key("themes/ocean.toml"));
    // The same pack as a package, a version on: an update in place, its checksum kept.
    let newer = theme_folder(work.path(), "pack", "1.1.0");
    let file = work.path().join("pack.tar.gz");
    std::fs::write(&file, pack(&newer)).unwrap();
    install::commit(
        data.path(),
        install::stage(data.path(), &file).unwrap(),
        Origin::Local,
        None,
    )
    .unwrap();
    let installed = &state::load(&plugins).plugins["pack"];
    assert_eq!(installed.version, "1.1.0");
    assert!(installed.sha256.as_ref().is_some_and(|sha| sha.len() == 64));
    assert_eq!(
        std::fs::read_dir(plugins.join(".staging")).unwrap().count(),
        0
    );
}

#[test]
fn disabling_and_removing_change_the_state() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let folder = theme_folder(work.path(), "pack", "1.0.0");
    install::commit(
        data.path(),
        install::stage(data.path(), &folder).unwrap(),
        Origin::Local,
        None,
    )
    .unwrap();
    let plugins = data.path().join("plugins");
    install::set_enabled(data.path(), "pack", false, None).unwrap();
    assert!(!state::load(&plugins).plugins["pack"].enabled);
    install::remove(data.path(), "pack", None).unwrap();
    assert!(!plugins.join("pack").exists());
    assert!(state::load(&plugins).plugins.is_empty());
    let error = install::remove(data.path(), "pack", None).unwrap_err();
    assert!(
        error.to_string().contains("no plugin 'pack' is installed"),
        "{error}"
    );
}

#[test]
fn the_state_is_rebuilt_from_the_folders() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let folder = theme_folder(work.path(), "pack", "1.0.0");
    install::commit(
        data.path(),
        install::stage(data.path(), &folder).unwrap(),
        Origin::Local,
        None,
    )
    .unwrap();
    let plugins = data.path().join("plugins");
    std::fs::write(plugins.join("state.json"), "{ truncated").unwrap();
    let rebuilt = state::load(&plugins);
    assert_eq!(rebuilt.plugins["pack"].version, "1.0.0");
    assert!(rebuilt.plugins["pack"].enabled);
    std::fs::remove_dir_all(plugins.join("pack")).unwrap();
    assert!(state::load(&plugins).plugins.is_empty());
}

#[test]
fn a_plugin_for_a_newer_dexo_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let folder = theme_folder(work.path(), "pack", "1.0.0");
    let manifest = folder.join("dexo-plugin.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        text.replace("min_dexo = \"1.0.0\"", "min_dexo = \"99.0.0\""),
    )
    .unwrap();
    // Refused as it is unpacked: before anyone is asked to trust it.
    let Err(error) = install::stage(data.path(), &folder) else {
        panic!("staged a plugin this Dexo cannot run");
    };
    assert!(
        error.to_string().contains("needs Dexo 99.0.0 or newer"),
        "{error}"
    );
    assert!(!data.path().join("plugins/pack").exists());
}

#[test]
fn a_held_lock_refuses_a_second_install_and_a_stale_one_is_taken_over() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let plugins = data.path().join("plugins");
    let held = Lock::acquire(&plugins, "pack").unwrap();
    let folder = theme_folder(work.path(), "pack", "1.0.0");
    let error = install::commit(
        data.path(),
        install::stage(data.path(), &folder).unwrap(),
        Origin::Local,
        None,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("another Dexo is installing or removing plugins"),
        "{error}"
    );
    std::mem::forget(held);
    let lock = plugins.join(".locks/pack.lock");
    let old = SystemTime::now() - Duration::from_secs(11 * 60);
    std::fs::File::options()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(old)
        .unwrap();
    install::commit(
        data.path(),
        install::stage(data.path(), &folder).unwrap(),
        Origin::Local,
        None,
    )
    .unwrap();
}

#[test]
fn a_package_with_a_file_a_plugin_does_not_ship_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let folder = theme_folder(work.path(), "pack", "1.0.0");
    std::fs::write(folder.join("install.sh"), "rm -rf ~").unwrap();
    let error = install::stage(data.path(), &folder).err().unwrap();
    assert!(error.to_string().contains("install.sh"), "{error}");
}
