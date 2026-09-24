use std::path::{Path, PathBuf};
use std::time::Duration;

use dexo_app::update::{self, UpdateCache};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::action::Action;

/// Past this the check gives up; it must never be why Dexo feels slow to start.
const TIMEOUT: Duration = Duration::from_secs(10);
/// The response is a redirect with no body; anything this large is not the answer.
const MAX_RESPONSE: usize = 16 * 1024;

/// Looks for a newer release in the background and reports one as an action. Off
/// when the user turned it off, or when `DEXO_NO_UPDATE_CHECK` is set (scripts, CI).
/// Every failure is silent: being offline is not the user's problem to read about.
pub fn spawn(data_dir: PathBuf, action_tx: tokio::sync::mpsc::Sender<Action>) {
    tokio::spawn(async move {
        if let Some(action) = check(&data_dir).await {
            let _ = action_tx.send(action).await;
        }
    });
}

async fn check(data_dir: &Path) -> Option<Action> {
    if std::env::var_os("DEXO_NO_UPDATE_CHECK").is_some()
        || !dexo_app::settings::load_settings(data_dir).update_check
    {
        return None;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let latest = match update::load_cache(data_dir) {
        Some(cache) if cache.is_fresh(now) => cache.latest,
        _ => {
            let latest = tokio::time::timeout(TIMEOUT, fetch_latest())
                .await
                .ok()?
                .ok()?;
            let _ = update::save_cache(
                data_dir,
                &UpdateCache {
                    checked_at: now,
                    latest: latest.clone(),
                },
            );
            latest
        }
    };
    if !update::is_newer(&latest, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    Some(Action::UpdateAvailable {
        command: update::update_command(running_install()).into(),
        version: latest,
    })
}

fn running_install() -> update::InstallMethod {
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .unwrap_or_default();
    let updater = if cfg!(windows) {
        "dexo-update.exe"
    } else {
        "dexo-update"
    };
    update::detect_install(&exe, exe.with_file_name(updater).exists())
}

async fn fetch_latest() -> Result<String, String> {
    let tcp = dexo_transport::connect_direct(update::HOST, 443)
        .await
        .map_err(|error| error.to_string())?;
    let tls = dexo_transport::TlsConfig {
        mode: dexo_transport::TlsMode::VerifyFull,
        explicit_insecure: false,
        server_name: update::HOST.into(),
        ca_file: None,
    };
    let mut stream = dexo_transport::connect_tls(tcp, &tls, None)
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(update::head_request(env!("CARGO_PKG_VERSION")).as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    let mut chunk = [0u8; 4096];
    while !response.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 || response.len() > MAX_RESPONSE {
            break;
        }
        response.extend_from_slice(&chunk[..read]);
    }
    update::latest_from_response(&String::from_utf8_lossy(&response))
        .ok_or_else(|| "releases/latest did not redirect to a release".into())
}

#[cfg(test)]
mod tests {
    /// Talks to github.com, so it stays out of the default run: `cargo test -p dexo-tui
    /// --lib update_check -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn github_names_the_latest_release() {
        let latest = super::fetch_latest()
            .await
            .expect("no answer from github.com");
        assert!(
            dexo_app::update::is_newer(&latest, "0.0.1"),
            "not a version: {latest}"
        );
    }
}
