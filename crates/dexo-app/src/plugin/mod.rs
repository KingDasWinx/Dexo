//! Plugins: packages that add drivers and themes to an installed Dexo.

pub mod archive;
pub mod host;
pub mod install;
pub mod manifest;
pub mod proxy;
pub mod rpc;
pub mod state;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dexo_driver_api::DriverDescriptor;

use crate::driver_registry::DriverRegistry;

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Drivers Dexo knows of that a local build provides through `DEXO_DRIVER_<ID>`:
/// developing the DuckDB plugin needs no package.
pub fn register_dev_overrides(registry: &DriverRegistry) {
    for descriptor in DriverDescriptor::known() {
        let Some(plugin) = descriptor.plugin.clone() else {
            continue;
        };
        if registry.has(&descriptor.id) {
            continue;
        }
        if let Some(executable) = host::dev_override(&plugin) {
            let host = host::PluginHost::new(
                descriptor.display_name.to_string(),
                vec![descriptor.id.to_string()],
                host::process_spawner(executable),
            );
            registry.insert(Arc::new(proxy::PluginFactory::new(
                descriptor,
                Arc::new(host),
            )));
        }
    }
}

/// A registered driver plugin's process, and the drivers it provides.
type Registered = (Arc<host::PluginHost>, Vec<String>);

/// The driver plugins registered in this process, by plugin id: what removing or
/// disabling one stops.
static HOSTS: Mutex<BTreeMap<String, Registered>> = Mutex::new(BTreeMap::new());

pub fn plugins_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plugins")
}

/// Registers installed plugin `id`'s drivers, when it is an enabled driver plugin.
pub fn register(registry: &DriverRegistry, data_dir: &Path, id: &str) {
    let plugins = plugins_dir(data_dir);
    if !state::load(&plugins)
        .plugins
        .get(id)
        .is_some_and(|installed| installed.enabled)
    {
        return;
    }
    let folder = plugins.join(id);
    let Some(manifest) = manifest::read(&folder) else {
        return;
    };
    let Some(driver) = &manifest.driver else {
        return;
    };
    let Some(executable) = host::dev_override(id).or_else(|| manifest.executable(&folder)) else {
        return;
    };
    let plugin = Arc::new(host::PluginHost::new(
        driver.display_name.clone(),
        driver.drivers.clone(),
        host::process_spawner(executable),
    ));
    for descriptor in manifest.descriptors() {
        registry.insert(Arc::new(proxy::PluginFactory::new(
            descriptor,
            plugin.clone(),
        )));
    }
    let replaced = lock(&HOSTS).insert(id.to_string(), (plugin, driver.drivers.clone()));
    if let Some((old, _)) = replaced {
        old.stop("it was replaced");
    }
}

/// Takes plugin `id`'s drivers out of the registry and stops its process, failing its
/// open sessions with `why`.
pub fn unregister(registry: &DriverRegistry, id: &str, why: &str) {
    let removed = lock(&HOSTS).remove(id);
    if let Some((plugin, drivers)) = removed {
        for driver in &drivers {
            registry.remove(driver);
        }
        plugin.stop(why);
    }
}

/// What installing a plugin from outside the signed index means, said before it does.
pub fn trust_text(manifest: &manifest::Manifest) -> String {
    let authors = if manifest.authors.is_empty() {
        "an unnamed author".to_string()
    } else {
        manifest.authors.join(", ")
    };
    let mut text = format!(
        "{} {} ({}, id {}) by {authors} is not from Dexo's signed index: nothing vouches for it.",
        manifest.name,
        manifest.version,
        manifest.kind.as_str(),
        manifest.id
    );
    if manifest.kind == manifest::Kind::Driver {
        text.push_str(
            " It is a driver: this program will receive the passwords of the connections that use it.",
        );
    }
    text
}

/// Every installed, enabled driver plugin's drivers, then the `DEXO_DRIVER_<ID>` builds
/// of drivers not installed: what Dexo connects with from its start.
pub fn register_installed(registry: &DriverRegistry, data_dir: &Path) {
    for (id, installed) in state::load(&plugins_dir(data_dir)).plugins {
        if installed.enabled && installed.kind == manifest::Kind::Driver {
            register(registry, data_dir, &id);
        }
    }
    register_dev_overrides(registry);
}

/// The themes folders of the enabled theme plugins, with their plugin's id.
pub fn theme_dirs(data_dir: &Path) -> Vec<(String, PathBuf)> {
    let plugins = plugins_dir(data_dir);
    state::load(&plugins)
        .plugins
        .into_iter()
        .filter(|(_, installed)| installed.enabled && installed.kind == manifest::Kind::Theme)
        .map(|(id, _)| {
            let themes = plugins.join(&id).join("themes");
            (id, themes)
        })
        .collect()
}
