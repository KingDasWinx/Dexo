//! The Plugins screen's work, off the loop that draws.

use std::path::Path;

use dexo_app::plugin::index::{self, IndexSource};
use dexo_app::plugin::{manifest, plugins_dir, state};

use crate::screens::plugins::{PluginRow, PluginsSnapshot};

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
