//! `plugins/state.json`: what is installed, from where, and whether it is enabled. The
//! folders are the truth; this is a record of them, rebuilt from their manifests when
//! it is missing or unreadable.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::manifest::{self, Kind, Manifest};
use crate::error::{AppError, ErrorCategory};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// From the signed index.
    Official,
    /// From a file, a URL or a folder: nothing vouches for it.
    Local,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    pub kind: Kind,
    pub enabled: bool,
    pub origin: Origin,
    /// The package's sha256, when it came as one.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Every file's sha256 by its path in the plugin's folder, as installed: what
    /// `dexo plugin doctor` checks the files against.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// Unix seconds.
    #[serde(default)]
    pub installed_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub plugins: BTreeMap<String, Installed>,
}

/// The installed plugins: `state.json` checked against the folders. A folder without an
/// entry is added -- local and enabled, as nothing says otherwise -- and an entry
/// without a folder is dropped.
pub fn load(plugins: &Path) -> State {
    let mut state: State = std::fs::read(plugins.join("state.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let on_disk = manifests(plugins);
    state
        .plugins
        .retain(|id, _| on_disk.iter().any(|(found, _)| found == id));
    for (id, manifest) in on_disk {
        state.plugins.entry(id).or_insert_with(|| Installed {
            version: manifest.version.clone(),
            kind: manifest.kind,
            enabled: true,
            origin: Origin::Local,
            sha256: None,
            files: BTreeMap::new(),
            installed_at: 0,
        });
    }
    state
}

/// The plugin folders whose manifest reads and names the folder, by id.
pub fn manifests(plugins: &Path) -> Vec<(String, Manifest)> {
    let Ok(entries) = std::fs::read_dir(plugins) else {
        return Vec::new();
    };
    let mut found: Vec<(String, Manifest)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let id = entry.file_name().to_str()?.to_string();
            if !manifest::valid_id(&id) {
                return None;
            }
            let manifest = manifest::read(&entry.path())?;
            (manifest.id == id).then_some((id, manifest))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Written through a temporary file and a rename: a crash mid-write leaves the old one.
pub fn save(plugins: &Path, state: &State) -> std::io::Result<()> {
    std::fs::create_dir_all(plugins)?;
    let temporary = plugins.join(format!("state.json.{}", std::process::id()));
    let text = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, plugins.join("state.json"))
}

/// Reads, changes and writes the state under its lock: the TUI and a `dexo plugin`
/// command may both be at it.
pub fn change<T>(plugins: &Path, edit: impl FnOnce(&mut State) -> T) -> Result<T, AppError> {
    let _lock = Lock::wait(plugins, ".state", Duration::from_secs(3))?;
    let mut state = load(plugins);
    let out = edit(&mut state);
    save(plugins, &state).map_err(|error| {
        AppError::new(
            ErrorCategory::Storage,
            format!("plugins/state.json could not be written: {error}"),
        )
    })?;
    Ok(out)
}

/// `plugins/.locks/<name>.lock`, made exclusively and holding this process's id; gone
/// when dropped.
pub struct Lock {
    path: PathBuf,
}

/// A lock this old is from a Dexo that died holding it.
const STALE: Duration = Duration::from_secs(10 * 60);

impl Lock {
    /// `acquire`, tried again for up to `patience`: for a lock held a moment, as the
    /// state file's is.
    pub fn wait(plugins: &Path, name: &str, patience: Duration) -> Result<Self, AppError> {
        let started = std::time::Instant::now();
        loop {
            match Self::acquire(plugins, name) {
                Err(error)
                    if error.category() == ErrorCategory::Conflict
                        && started.elapsed() < patience =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                done => return done,
            }
        }
    }

    pub fn acquire(plugins: &Path, name: &str) -> Result<Self, AppError> {
        let locks = plugins.join(".locks");
        std::fs::create_dir_all(&locks).map_err(|error| {
            AppError::new(
                ErrorCategory::Storage,
                format!("{}: {error}", locks.display()),
            )
        })?;
        let path = locks.join(format!("{name}.lock"));
        for _ in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let _ = write!(file, "{}", std::process::id());
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let age = std::fs::metadata(&path)
                        .and_then(|metadata| metadata.modified())
                        .ok()
                        .and_then(|modified| modified.elapsed().ok());
                    if age.is_some_and(|age| age > STALE) {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    break;
                }
                Err(error) => {
                    return Err(AppError::new(
                        ErrorCategory::Storage,
                        format!("{}: {error}", path.display()),
                    ));
                }
            }
        }
        Err(AppError::new(
            ErrorCategory::Conflict,
            "another Dexo is installing or removing plugins right now: try again in a moment",
        ))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
