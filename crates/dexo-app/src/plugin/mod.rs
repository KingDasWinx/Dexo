//! Plugins: packages that add drivers and themes to an installed Dexo.

pub mod host;
pub mod proxy;
pub mod rpc;

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
