use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use dexo_driver_api::{ConnectionFactory, DriverDescriptor};

use crate::error::{AppError, ErrorCategory};

/// The drivers Dexo connects with. Clones share one table: a driver plugin installed
/// from the Plugins screen is there for every part of Dexo without a restart.
#[derive(Clone, Default)]
pub struct DriverRegistry {
    factories: Arc<RwLock<HashMap<String, Arc<dyn ConnectionFactory>>>>,
}

impl DriverRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, factory: Arc<dyn ConnectionFactory>) {
        self.insert(factory);
    }

    /// Adds a driver; a plugin's is announced as installed, so the forms list it so.
    pub fn insert(&self, factory: Arc<dyn ConnectionFactory>) {
        let descriptor = factory.descriptor();
        if descriptor.plugin.is_some() {
            DriverDescriptor::announce(DriverDescriptor {
                installed: true,
                ..descriptor.clone()
            });
        }
        self.factories
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(descriptor.id.into_owned(), factory);
    }

    /// Takes a driver out; a plugin's stays known, as not installed.
    pub fn remove(&self, driver: &str) -> Option<Arc<dyn ConnectionFactory>> {
        let removed = self
            .factories
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(driver)?;
        let descriptor = removed.descriptor();
        if descriptor.plugin.is_some() {
            DriverDescriptor::announce(DriverDescriptor {
                installed: false,
                ..descriptor
            });
        }
        Some(removed)
    }

    pub fn has(&self, driver: &str) -> bool {
        self.factories
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(driver)
    }

    /// A plugin's driver that is not installed says how to get it.
    pub fn get(&self, driver: &str) -> Result<Arc<dyn ConnectionFactory>, AppError> {
        if let Some(factory) = self
            .factories
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(driver)
        {
            return Ok(factory.clone());
        }
        Err(
            match DriverDescriptor::for_id(driver).and_then(|descriptor| {
                let plugin = descriptor.plugin.clone()?;
                Some((descriptor, plugin))
            }) {
                Some((descriptor, plugin)) => AppError::new(
                    ErrorCategory::Capability,
                    format!(
                        "{} is a plugin: install it with `dexo plugin install {plugin}` or from Plugins (Ctrl+G p)",
                        descriptor.display_name
                    ),
                ),
                None => AppError::new(
                    ErrorCategory::Configuration,
                    format!("unknown driver '{driver}'"),
                ),
            },
        )
    }

    pub fn descriptors(&self) -> Vec<DriverDescriptor> {
        let mut descriptors: Vec<_> = self
            .factories
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|factory| factory.descriptor())
            .collect();
        descriptors.sort_by(|a, b| a.id.cmp(&b.id));
        descriptors
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dexo_driver_api::{
        ConnectRequest, ConnectionFactory, DriverDescriptor, DriverError, Session,
    };

    use super::DriverRegistry;
    use crate::error::ErrorCategory;

    struct Fake;

    #[async_trait::async_trait]
    impl ConnectionFactory for Fake {
        fn descriptor(&self) -> DriverDescriptor {
            DriverDescriptor::plugin_driver("registry-fake", "Fake", true, "registry-fake", false)
        }

        async fn connect(&self, _: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
            Err(DriverError::unsupported("fake"))
        }
    }

    #[test]
    fn a_plugin_driver_not_installed_says_how_to_get_it() {
        let registry = DriverRegistry::new();
        let missing = registry.get("duckdb").err().unwrap();
        assert_eq!(missing.category(), ErrorCategory::Capability);
        assert!(
            missing.to_string().contains(
                "DuckDB is a plugin: install it with `dexo plugin install duckdb` or from Plugins (Ctrl+G p)"
            ),
            "{missing}"
        );
        let unknown = registry.get("oracle").err().unwrap();
        assert_eq!(unknown.category(), ErrorCategory::Configuration);
        // Postgres is built in: a registry without it simply lacks it.
        let unregistered = registry.get("postgres").err().unwrap();
        assert_eq!(unregistered.category(), ErrorCategory::Configuration);
    }

    #[test]
    fn clones_share_their_drivers_and_announce_them() {
        let registry = DriverRegistry::new();
        let clone = registry.clone();
        clone.insert(Arc::new(Fake));
        assert!(registry.get("registry-fake").is_ok());
        assert!(DriverDescriptor::for_id("registry-fake").unwrap().installed);
        assert!(registry.remove("registry-fake").is_some());
        assert!(!clone.has("registry-fake"));
        assert!(!DriverDescriptor::for_id("registry-fake").unwrap().installed);
        DriverDescriptor::withdraw("registry-fake");
    }
}
