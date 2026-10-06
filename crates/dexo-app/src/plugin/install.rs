//! Putting plugins in place and taking them out. Every change goes through staging and
//! a rename, so a failure at any step leaves the plugin as it was.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use dexo_plugin_protocol::PROTOCOL;
use sha2::{Digest, Sha256};

use super::host::{self, PluginHost};
use super::index::{self, IndexSource};
use super::manifest::{self, Manifest};
use super::state::{self, Installed, Lock, Origin};
use super::{archive, http, plugins_dir, register, unregister};
use crate::driver_registry::DriverRegistry;
use crate::error::{AppError, ErrorCategory};

/// A package unpacked into `plugins/.staging/`, its manifest read and checked: what the
/// trust prompt shows before a local install goes on. Dropped, its folder goes.
pub struct Staged {
    pub manifest: Manifest,
    /// The package's sha256; none for a folder.
    pub sha256: Option<String>,
    dir: Staging,
}

impl Staged {
    pub fn path(&self) -> &Path {
        &self.dir.0
    }
}

struct Staging(PathBuf);

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn staging(plugins: &Path, label: &str) -> Result<Staging, AppError> {
    let path = plugins
        .join(".staging")
        .join(format!("{label}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&path).map_err(|error| storage("a staging folder", error))?;
    Ok(Staging(path))
}

fn storage(what: &str, error: std::io::Error) -> AppError {
    AppError::new(ErrorCategory::Storage, format!("{what}: {error}"))
}

fn refused(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCategory::Configuration, message)
}

fn not_installed(id: &str) -> AppError {
    refused(format!("no plugin '{id}' is installed"))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Unpacks a package -- a `.tar.gz` file or a plugin's folder -- into staging; refused
/// when this Dexo cannot run it.
pub fn stage(data_dir: &Path, source: &Path) -> Result<Staged, AppError> {
    if source.is_dir() {
        let dir = staging(&plugins_dir(data_dir), "local")?;
        copy_package(source, &dir.0)?;
        return finish_stage(data_dir, dir, None);
    }
    let unreadable = |error: std::io::Error| refused(format!("{}: {error}", source.display()));
    let size = std::fs::metadata(source).map_err(unreadable)?.len();
    if size > DOWNLOAD_LIMIT {
        return Err(refused(format!(
            "{} is larger than a plugin may be ({} MB)",
            source.display(),
            DOWNLOAD_LIMIT / (1024 * 1024)
        )));
    }
    let bytes = std::fs::read(source).map_err(unreadable)?;
    stage_bytes(data_dir, &bytes)
}

pub fn stage_bytes(data_dir: &Path, bytes: &[u8]) -> Result<Staged, AppError> {
    let dir = staging(&plugins_dir(data_dir), "package")?;
    archive::unpack(bytes, &dir.0).map_err(refused)?;
    finish_stage(data_dir, dir, Some(hex(&Sha256::digest(bytes))))
}

fn finish_stage(data_dir: &Path, dir: Staging, sha256: Option<String>) -> Result<Staged, AppError> {
    let text = std::fs::read_to_string(dir.0.join("dexo-plugin.toml"))
        .map_err(|_| refused("the package has no dexo-plugin.toml"))?;
    let manifest = manifest::parse(&text).map_err(refused)?;
    archive::check_kind(&dir.0, &manifest).map_err(refused)?;
    let dexo = env!("CARGO_PKG_VERSION");
    if crate::update::is_newer(&manifest.min_dexo, dexo) {
        return Err(refused(format!(
            "{} {} needs Dexo {} or newer; this is {dexo}",
            manifest.name, manifest.version, manifest.min_dexo
        )));
    }
    if let Some(driver) = &manifest.driver
        && driver.protocol != PROTOCOL
    {
        return Err(refused(format!(
            "{} speaks plugin protocol {}, and this Dexo speaks {PROTOCOL}",
            manifest.name, driver.protocol
        )));
    }
    if let Some((other, driver)) = brought_elsewhere(data_dir, &manifest) {
        return Err(refused(format!(
            "the plugin {other} already brings the driver '{driver}'"
        )));
    }
    Ok(Staged {
        manifest,
        sha256,
        dir,
    })
}

/// A driver `manifest` brings that another installed plugin brings already, and that
/// plugin: two would take each other's connections.
fn brought_elsewhere(data_dir: &Path, manifest: &Manifest) -> Option<(String, String)> {
    let mine = &manifest.driver.as_ref()?.drivers;
    let plugins = plugins_dir(data_dir);
    state::load(&plugins)
        .plugins
        .into_keys()
        .filter(|other| *other != manifest.id)
        .find_map(|other| {
            let theirs = manifest::read(&plugins.join(&other))?.driver?.drivers;
            let driver = mine.iter().find(|driver| theirs.contains(driver))?.clone();
            Some((other, driver))
        })
}

/// Copies a plugin's folder by the rules a package's files follow: no links, nothing a
/// plugin does not ship.
fn copy_package(from: &Path, to: &Path) -> Result<(), AppError> {
    for file in files(from)? {
        let name = relative(from, &file);
        if std::fs::symlink_metadata(&file)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(true)
            || !archive::allowed(&name)
        {
            return Err(refused(format!(
                "the folder holds '{name}', which a plugin does not ship"
            )));
        }
        let target = to.join(&name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| storage(&name, error))?;
        }
        std::fs::copy(&file, &target).map_err(|error| storage(&name, error))?;
    }
    Ok(())
}

/// The files under `dir`, at any depth.
fn files(dir: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut found = Vec::new();
    let entries =
        std::fs::read_dir(dir).map_err(|error| storage(&dir.display().to_string(), error))?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            found.extend(files(&path)?);
        } else {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn hash_files(dir: &Path) -> Result<BTreeMap<String, String>, AppError> {
    files(dir)?
        .into_iter()
        .map(|file| {
            let bytes =
                std::fs::read(&file).map_err(|error| storage(&relative(dir, &file), error))?;
            Ok((relative(dir, &file), hex(&Sha256::digest(&bytes))))
        })
        .collect()
}

/// Starts a driver plugin's executable and checks its handshake: it runs on this
/// platform and speaks this Dexo's protocol. A theme pack has nothing to run.
pub(crate) fn check_handshake(manifest: &Manifest, dir: &Path) -> Result<(), AppError> {
    let (Some(driver), Some(executable)) = (&manifest.driver, manifest.executable(dir)) else {
        return Ok(());
    };
    let plugin = PluginHost::new(
        driver.display_name.clone(),
        driver.drivers.clone(),
        host::process_spawner(executable),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::new(ErrorCategory::Internal, error.to_string()))?;
    let started = runtime.block_on(plugin.live());
    plugin.stop("its check is over");
    started.map(drop).map_err(|error| {
        AppError::new(
            ErrorCategory::ExternalTool,
            format!(
                "{} {} does not run here: {error}",
                manifest.name, manifest.version
            ),
        )
    })
}

/// Puts a staged plugin in place: a driver's handshake checked first, the old version moved aside only once the new one is ready,
/// the state written last.
pub fn commit(
    data_dir: &Path,
    staged: Staged,
    origin: Origin,
    registry: Option<&DriverRegistry>,
) -> Result<Manifest, AppError> {
    let plugins = plugins_dir(data_dir);
    let manifest = staged.manifest.clone();
    let _lock = Lock::acquire(&plugins, &manifest.id)?;
    check_handshake(&manifest, staged.path())?;
    let files = hash_files(staged.path())?;
    // Stopped first: Windows does not rename a folder whose program is running.
    if let Some(registry) = registry {
        unregister(registry, &manifest.id, "it is being updated");
    }
    if let Err(error) = swap(&plugins, staged.path(), &manifest.id) {
        if let Some(registry) = registry {
            register(registry, data_dir, &manifest.id);
        }
        return Err(error);
    }
    state::change(&plugins, |state| {
        let enabled = state
            .plugins
            .get(&manifest.id)
            .is_none_or(|old| old.enabled);
        state.plugins.insert(
            manifest.id.clone(),
            Installed {
                version: manifest.version.clone(),
                kind: manifest.kind,
                enabled,
                origin,
                sha256: staged.sha256.clone(),
                files,
                installed_at: now(),
            },
        );
    })?;
    if let Some(registry) = registry {
        register(registry, data_dir, &manifest.id);
    }
    Ok(manifest)
}

/// Moves `new` into `plugins/<id>`. What was there is set aside first and put back if
/// the move fails.
fn swap(plugins: &Path, new: &Path, id: &str) -> Result<(), AppError> {
    let target = plugins.join(id);
    let aside = plugins
        .join(".staging")
        .join(format!("{id}-old-{}", uuid::Uuid::new_v4().simple()));
    let had = target.exists();
    if had {
        std::fs::rename(&target, &aside).map_err(|error| {
            storage(
                &format!("the installed {id} could not be moved aside; is a Dexo using it?"),
                error,
            )
        })?;
    }
    if let Err(error) = std::fs::rename(new, &target) {
        if had {
            let _ = std::fs::rename(&aside, &target);
        }
        return Err(storage(&format!("{id} could not be put in place"), error));
    }
    if had {
        let _ = std::fs::remove_dir_all(&aside);
    }
    Ok(())
}

/// Deletes a plugin's folder and its entry. Connections that used its driver stay
/// saved, and say the driver is not installed.
pub fn remove(
    data_dir: &Path,
    id: &str,
    registry: Option<&DriverRegistry>,
) -> Result<(), AppError> {
    let plugins = plugins_dir(data_dir);
    let target = plugins.join(id);
    if !manifest::valid_id(id) || !target.is_dir() {
        return Err(not_installed(id));
    }
    let _lock = Lock::acquire(&plugins, id)?;
    if let Some(registry) = registry {
        unregister(registry, id, "it was removed");
    }
    let manifest = manifest::read(&target);
    let aside = plugins
        .join(".staging")
        .join(format!("{id}-removed-{}", uuid::Uuid::new_v4().simple()));
    let moved = std::fs::create_dir_all(plugins.join(".staging"))
        .and_then(|()| std::fs::rename(&target, &aside));
    if let Err(error) = moved {
        if let Some(registry) = registry {
            register(registry, data_dir, id);
        }
        return Err(storage(
            &format!("{id} could not be removed; is a Dexo using it?"),
            error,
        ));
    }
    let _ = std::fs::remove_dir_all(&aside);
    // A disabled plugin's drivers were known as disabled: they are not installed now.
    if let Some(manifest) = &manifest {
        super::announce_drivers(manifest, false);
    }
    state::change(&plugins, |state| {
        state.plugins.remove(id);
    })
}

/// A disabled plugin stays installed; its drivers and themes are not registered.
pub fn set_enabled(
    data_dir: &Path,
    id: &str,
    enabled: bool,
    registry: Option<&DriverRegistry>,
) -> Result<(), AppError> {
    let found = state::change(&plugins_dir(data_dir), |state| {
        state
            .plugins
            .get_mut(id)
            .map(|installed| installed.enabled = enabled)
            .is_some()
    })?;
    if !found {
        return Err(not_installed(id));
    }
    if let Some(registry) = registry {
        if !enabled {
            unregister(registry, id, "it was disabled");
        }
        register(registry, data_dir, id);
    }
    Ok(())
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The most a plugin package may weigh.
pub const DOWNLOAD_LIMIT: u64 = 256 * 1024 * 1024;

/// Installs `id` from the signed index: the newest version this Dexo runs, or `version`.
/// The download is checked against the index's size and sha256 before anything is
/// unpacked, and the package against the index entry once it is.
pub fn install_from_index(
    data_dir: &Path,
    source: &IndexSource,
    id: &str,
    version: Option<&str>,
    registry: Option<&DriverRegistry>,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<Manifest, AppError> {
    let fetched = index::load(data_dir, source, false)?;
    let choice = index::choose(
        &fetched.index,
        id,
        version,
        env!("CARGO_PKG_VERSION"),
        index::target(),
    )
    .map_err(refused)?;
    let artifact = choice.artifact;
    let name = format!("{} {}", choice.plugin.name, choice.version.version);
    if artifact.size > DOWNLOAD_LIMIT {
        return Err(refused(format!(
            "{name} is {} bytes, more than the 256 MB a plugin may be",
            artifact.size
        )));
    }
    let bytes = http::get(&artifact.url, artifact.size, progress, cancel).map_err(|error| {
        if error == "cancelled" {
            AppError::new(
                ErrorCategory::Cancelled,
                "the download was cancelled: nothing was installed",
            )
        } else {
            AppError::new(
                ErrorCategory::Network,
                format!("{error}: nothing was installed"),
            )
        }
    })?;
    if bytes.len() as u64 != artifact.size {
        return Err(refused(format!(
            "the download of {name} is {} bytes and the index says {}: nothing was installed",
            bytes.len(),
            artifact.size
        )));
    }
    if !hex(&Sha256::digest(&bytes)).eq_ignore_ascii_case(&artifact.sha256) {
        return Err(refused(format!(
            "the download of {name} does not match the index's checksum: nothing was installed"
        )));
    }
    let staged = stage_bytes(data_dir, &bytes)?;
    let manifest = &staged.manifest;
    if manifest.id != choice.plugin.id
        || manifest.version != choice.version.version
        || manifest.kind != choice.plugin.kind
    {
        return Err(refused(format!(
            "the package of {name} does not match its index entry: nothing was installed"
        )));
    }
    commit(data_dir, staged, Origin::Official, registry)
}

/// Updates `id` to the newest version this Dexo runs; `Ok(None)` when it has it.
pub fn upgrade(
    data_dir: &Path,
    source: &IndexSource,
    id: &str,
    registry: Option<&DriverRegistry>,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<Option<Manifest>, AppError> {
    let installed = state::load(&plugins_dir(data_dir))
        .plugins
        .get(id)
        .cloned()
        .ok_or_else(|| not_installed(id))?;
    if installed.origin == Origin::Local {
        return Err(refused(format!(
            "{id} was installed from a file: install the newer file to update it"
        )));
    }
    let fetched = index::load(data_dir, source, false)?;
    let newest = index::choose(
        &fetched.index,
        id,
        None,
        env!("CARGO_PKG_VERSION"),
        index::target(),
    )
    .map_err(refused)?
    .version
    .version
    .clone();
    if !crate::update::is_newer(&newest, &installed.version) {
        return Ok(None);
    }
    install_from_index(
        data_dir,
        source,
        id,
        Some(&newest),
        registry,
        progress,
        cancel,
    )
    .map(Some)
}

/// One installed plugin, checked.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Checkup {
    pub id: String,
    pub version: String,
    pub problems: Vec<String>,
}

/// Each installed plugin checked: its files against the checksums taken when it was
/// installed, whether this Dexo runs it, and an enabled driver's handshake.
pub fn doctor(data_dir: &Path) -> Vec<Checkup> {
    let plugins = plugins_dir(data_dir);
    state::load(&plugins)
        .plugins
        .into_iter()
        .map(|(id, installed)| {
            let folder = plugins.join(&id);
            let mut problems = Vec::new();
            for (file, sha256) in &installed.files {
                match std::fs::read(folder.join(file)) {
                    Ok(bytes) if hex(&Sha256::digest(&bytes)) == *sha256 => {}
                    Ok(_) => problems.push(format!("{file} was changed since it was installed")),
                    Err(_) => problems.push(format!("{file} is missing")),
                }
            }
            match manifest::read(&folder) {
                None => problems.push("its dexo-plugin.toml cannot be read".into()),
                Some(manifest) => {
                    if crate::update::is_newer(&manifest.min_dexo, env!("CARGO_PKG_VERSION")) {
                        problems.push(format!("it needs Dexo {} or newer", manifest.min_dexo));
                    }
                    if installed.enabled
                        && let Err(error) = check_handshake(&manifest, &folder)
                    {
                        problems.push(error.to_string());
                    }
                }
            }
            Checkup {
                id,
                version: installed.version,
                problems,
            }
        })
        .collect()
}
