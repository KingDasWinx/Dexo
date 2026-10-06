//! Putting plugins in place and taking them out. Every change goes through staging and
//! a rename, so a failure at any step leaves the plugin as it was.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dexo_plugin_protocol::PROTOCOL;
use sha2::{Digest, Sha256};

use super::host::{self, PluginHost};
use super::manifest::{self, Manifest};
use super::state::{self, Installed, Lock, Origin};
use super::{archive, plugins_dir, register, unregister};
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

/// Unpacks a package -- a `.tar.gz` file or a plugin's folder -- into staging.
pub fn stage(data_dir: &Path, source: &Path) -> Result<Staged, AppError> {
    if source.is_dir() {
        let dir = staging(&plugins_dir(data_dir), "local")?;
        copy_package(source, &dir.0)?;
        return finish_stage(dir, None);
    }
    let bytes =
        std::fs::read(source).map_err(|error| refused(format!("{}: {error}", source.display())))?;
    stage_bytes(data_dir, &bytes)
}

pub fn stage_bytes(data_dir: &Path, bytes: &[u8]) -> Result<Staged, AppError> {
    let dir = staging(&plugins_dir(data_dir), "package")?;
    archive::unpack(bytes, &dir.0).map_err(refused)?;
    finish_stage(dir, Some(hex(&Sha256::digest(bytes))))
}

fn finish_stage(dir: Staging, sha256: Option<String>) -> Result<Staged, AppError> {
    let text = std::fs::read_to_string(dir.0.join("dexo-plugin.toml"))
        .map_err(|_| refused("the package has no dexo-plugin.toml"))?;
    let manifest = manifest::parse(&text).map_err(refused)?;
    archive::check_kind(&dir.0, &manifest).map_err(refused)?;
    Ok(Staged {
        manifest,
        sha256,
        dir,
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

/// Puts a staged plugin in place: refused when this Dexo cannot run it, a driver's
/// handshake checked first, the old version moved aside only once the new one is ready,
/// the state written last.
pub fn commit(
    data_dir: &Path,
    staged: Staged,
    origin: Origin,
    registry: Option<&DriverRegistry>,
) -> Result<Manifest, AppError> {
    let plugins = plugins_dir(data_dir);
    let manifest = staged.manifest.clone();
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
        if enabled {
            register(registry, data_dir, id);
        } else {
            unregister(registry, id, "it was disabled");
        }
    }
    Ok(())
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}
