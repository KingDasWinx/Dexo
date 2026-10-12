//! The Plugins screen's work, off the loop that draws.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use dexo_app::plugin::index::{self, IndexSource};
use dexo_app::plugin::install;
use dexo_app::plugin::state::Origin;
use dexo_app::plugin::{manifest, plugins_dir, state};
use dexo_app::{AppError, DriverRegistry, ErrorCategory};
use dexo_storage::AppPaths;

use crate::action::Action;
use crate::screens::plugins::{PluginRow, PluginsSnapshot, StagedInfo};

/// What the screen shows: every installed plugin with what its manifest says, and the
/// index's plugins with their newest version this Dexo runs. The index is the cache
/// within the day unless `refresh`. The index's drivers not installed are announced,
/// so the connection form offers them.
pub fn snapshot(data_dir: &Path, source: &IndexSource, refresh: bool) -> PluginsSnapshot {
    let plugins = plugins_dir(data_dir);
    let mut rows: Vec<PluginRow> = state::load(&plugins)
        .plugins
        .into_iter()
        .filter_map(|(id, installed)| {
            let folder = plugins.join(&id);
            let manifest = manifest::read(&folder)?;
            Some(PluginRow {
                provides: manifest::provides(&manifest, &folder),
                readme: readme(&folder),
                name: manifest.name.clone(),
                kind: manifest.kind,
                description: manifest.description.clone(),
                authors: manifest.authors.clone(),
                license: manifest.license.clone(),
                min_dexo: Some(manifest.min_dexo.clone()),
                protocol: manifest.driver.as_ref().map(|driver| driver.protocol),
                installed: Some(installed),
                newest: None,
                size: None,
                id,
            })
        })
        .collect();
    let mut snapshot = PluginsSnapshot::default();
    match index::load(data_dir, source, refresh) {
        Ok(fetched) => {
            if fetched.offline {
                snapshot.offline = Some(index::offline_note(fetched.fetched_at));
            }
            index::announce_available(&fetched.index);
            let dexo = env!("CARGO_PKG_VERSION");
            for plugin in &fetched.index.plugins {
                let Ok(choice) =
                    index::choose(&fetched.index, &plugin.id, None, dexo, index::target())
                else {
                    continue;
                };
                match rows.iter_mut().find(|row| row.id == plugin.id) {
                    Some(row) => {
                        row.newest = Some(choice.version.version.clone());
                        row.size = Some(choice.artifact.size);
                    }
                    None => rows.push(PluginRow {
                        id: plugin.id.clone(),
                        name: plugin.name.clone(),
                        kind: plugin.kind,
                        description: plugin.description.clone(),
                        authors: plugin.authors.clone(),
                        license: plugin.license.clone(),
                        installed: None,
                        newest: Some(choice.version.version.clone()),
                        min_dexo: Some(choice.version.min_dexo.clone()),
                        protocol: choice.version.driver.as_ref().map(|driver| driver.protocol),
                        size: Some(choice.artifact.size),
                        provides: choice
                            .version
                            .driver
                            .as_ref()
                            .map(|driver| driver.drivers.clone())
                            .unwrap_or_default(),
                        readme: Vec::new(),
                    }),
                }
            }
        }
        Err(error) => snapshot.index_error = Some(error.to_string()),
    }
    rows.sort_by_key(|row| row.name.to_lowercase());
    snapshot.rows = rows;
    snapshot
}

/// The first lines of a plugin's README.
fn readme(folder: &Path) -> Vec<String> {
    std::fs::read_to_string(folder.join("README.md"))
        .map(|text| text.lines().take(12).map(str::to_string).collect())
        .unwrap_or_default()
}

impl super::WorkbenchRuntime {
    /// Runs `work` on a blocking thread with the data folder and the drivers -- a
    /// driver plugin installed here is registered for the whole of Dexo at once -- says
    /// how it went, and reads the screen's state again.
    pub(super) fn plugin_work(
        &self,
        work: impl FnOnce(&Path, &DriverRegistry) -> Result<String, AppError> + Send + 'static,
    ) {
        let registry = self.drivers.clone();
        self.off_the_loop(move || {
            let Ok(paths) = AppPaths::discover() else {
                return Vec::new();
            };
            let done = work(&paths.data_dir, &registry).map_err(|error| error.to_string());
            vec![
                Action::PluginWorkDone(done),
                Action::PluginsLoaded(snapshot(&paths.data_dir, &IndexSource::from_env(), false)),
            ]
        });
    }

    pub(super) fn install_plugin(&mut self, id: String, upgrade: bool) {
        let cancel = Arc::new(AtomicBool::new(false));
        self.plugin_cancel = Some(cancel.clone());
        let progress_tx = self.action_tx.clone();
        self.plugin_work(move |data_dir, registry| {
            let mut last: Option<Instant> = None;
            // A tenth of a second between reports: the screen does not need more.
            let mut progress = |done, total| {
                if last.is_none_or(|at| at.elapsed() >= Duration::from_millis(100)) {
                    last = Some(Instant::now());
                    let _ = progress_tx.blocking_send(Action::PluginProgress { done, total });
                }
            };
            let source = IndexSource::from_env();
            if upgrade {
                Ok(
                    match install::upgrade(
                        data_dir,
                        &source,
                        &id,
                        Some(registry),
                        &mut progress,
                        &cancel,
                    )? {
                        Some(manifest) => {
                            format!("Updated {} to {}.", manifest.name, manifest.version)
                        }
                        None => format!("{id} is up to date."),
                    },
                )
            } else {
                let manifest = install::install_from_index(
                    data_dir,
                    &source,
                    &id,
                    None,
                    Some(registry),
                    &mut progress,
                    &cancel,
                )?;
                Ok(format!("Installed {} {}.", manifest.name, manifest.version))
            }
        });
    }

    pub(super) fn stage_plugin_file(&mut self, path: String) {
        self.next_staged += 1;
        let token = self.next_staged;
        let staged_plugins = self.staged_plugins.clone();
        self.off_the_loop(move || {
            let Ok(paths) = AppPaths::discover() else {
                return Vec::new();
            };
            let staged = install::stage(&paths.data_dir, &expand_home(&path))
                .map(|staged| {
                    let info = StagedInfo {
                        token,
                        id: staged.manifest.id.clone(),
                        trust: dexo_app::plugin::trust_text(&staged.manifest),
                    };
                    staged_plugins
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(token, staged);
                    info
                })
                .map_err(|error| error.to_string());
            vec![Action::PluginFileStaged(staged)]
        });
    }

    pub(super) fn commit_plugin_file(&mut self, token: u64) {
        let staged = self
            .staged_plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&token);
        self.plugin_work(move |data_dir, registry| {
            let staged = staged.ok_or_else(|| {
                AppError::new(
                    ErrorCategory::Configuration,
                    "the package is gone: choose it again",
                )
            })?;
            let manifest = install::commit(data_dir, staged, Origin::Local, Some(registry))?;
            Ok(format!(
                "Installed {} {} (local, not verified).",
                manifest.name, manifest.version
            ))
        });
    }
}

/// `~/x` as the home folder's `x`: a path typed by hand.
fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map_or_else(
                || PathBuf::from(path),
                |home| PathBuf::from(home).join(rest),
            ),
        None => PathBuf::from(path),
    }
}
