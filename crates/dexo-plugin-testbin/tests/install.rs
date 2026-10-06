//! Installing and removing a driver plugin: its handshake is checked before it goes in
//! place, and removing it ends its sessions cleanly. One test at a time: the plugin's
//! process is registered process-wide under its id.
use std::path::Path;
use std::sync::Mutex;

use dexo_app::DriverRegistry;
use dexo_app::plugin::install;
use dexo_app::plugin::state::{self, Origin};
use dexo_driver_api::{ConnectRequest, DbValue, QueryEvent, QueryRequest, Session};
use dexo_test_support::plugins::driver_folder;
use futures_util::StreamExt;
use secrecy::SecretString;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn testbin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_dexo-plugin-testbin"))
}

fn request(dir: &Path) -> ConnectRequest {
    ConnectRequest::new(
        dir.join("t.db").to_string_lossy(),
        None,
        String::new(),
        SecretString::from(String::new()),
        false,
    )
}

async fn run(
    session: &dyn Session,
    sql: &str,
) -> Result<Vec<Vec<DbValue>>, dexo_driver_api::DriverError> {
    let mut stream = session.execute(QueryRequest::read(sql, 0)).await?;
    let mut rows = Vec::new();
    while let Some(event) = stream.next().await {
        if let QueryEvent::Rows(batch) = event? {
            rows.extend(batch.rows);
        }
    }
    Ok(rows)
}

/// Review focus 3.
#[test]
fn a_failed_update_keeps_the_previous_version() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let v1 = driver_folder(work.path(), "testbin", "1.0.0", testbin(), "plain");
    install::commit(
        data.path(),
        install::stage(data.path(), &v1).unwrap(),
        Origin::Local,
        None,
    )
    .unwrap();
    let v2 = driver_folder(work.path(), "testbin", "1.1.0", testbin(), "bad-protocol");
    let error = install::commit(
        data.path(),
        install::stage(data.path(), &v2).unwrap(),
        Origin::Local,
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("protocol 99"), "{error}");
    let plugins = data.path().join("plugins");
    let manifest = std::fs::read_to_string(plugins.join("testbin/dexo-plugin.toml")).unwrap();
    assert!(manifest.contains("version = \"1.0.0\""), "{manifest}");
    assert_eq!(state::load(&plugins).plugins["testbin"].version, "1.0.0");
    assert_eq!(
        std::fs::read_dir(plugins.join(".staging")).unwrap().count(),
        0
    );
}

/// Review focus 5.
#[test]
fn removing_a_plugin_ends_its_sessions_cleanly() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let folder = driver_folder(work.path(), "testbin", "1.0.0", testbin(), "plain");
    let registry = DriverRegistry::new();
    install::commit(
        data.path(),
        install::stage(data.path(), &folder).unwrap(),
        Origin::Local,
        Some(&registry),
    )
    .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let session = registry
            .get("testbin")
            .unwrap()
            .connect(request(work.path()))
            .await
            .unwrap();
        assert_eq!(
            run(&*session, "SELECT 1").await.unwrap(),
            vec![vec![DbValue::I64(1)]]
        );
        install::remove(data.path(), "testbin", Some(&registry)).unwrap();
        let error = run(&*session, "SELECT 1").await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "the Testbin plugin stopped: it was removed"
        );
    });
    let missing = registry.get("testbin").err().unwrap();
    assert!(
        missing
            .to_string()
            .contains("Testbin is a plugin: install it with `dexo plugin install testbin`"),
        "{missing}"
    );
    assert!(!data.path().join("plugins/testbin").exists());
}
