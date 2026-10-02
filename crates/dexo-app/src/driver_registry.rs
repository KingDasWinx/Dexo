use std::collections::HashMap;
use std::sync::Arc;

use dexo_driver_api::ConnectionFactory;

use crate::error::{AppError, ErrorCategory};

/// Drivers built in only with the cargo feature of the same name.
const FEATURE_GATED: &[&str] = &["duckdb"];

#[derive(Clone, Default)]
pub struct DriverRegistry {
    factories: HashMap<&'static str, Arc<dyn ConnectionFactory>>,
}

impl DriverRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, factory: Arc<dyn ConnectionFactory>) {
        self.factories.insert(factory.driver_name(), factory);
    }

    /// A driver Dexo has but this build left out -- DuckDB, built in only with its cargo
    /// feature -- says how to get it.
    pub fn get(&self, driver: &str) -> Result<Arc<dyn ConnectionFactory>, AppError> {
        self.factories.get(driver).cloned().ok_or_else(|| {
            match dexo_driver_api::DriverDescriptor::for_id(driver)
                .filter(|descriptor| FEATURE_GATED.contains(&descriptor.id))
            {
                Some(descriptor) => AppError::new(
                    ErrorCategory::Capability,
                    format!(
                        "this build of Dexo has no {} driver: build it with `--features {}`",
                        descriptor.display_name, descriptor.id
                    ),
                ),
                None => AppError::new(
                    ErrorCategory::Configuration,
                    format!("unknown driver '{driver}'"),
                ),
            }
        })
    }

    pub fn descriptors(&self) -> Vec<dexo_driver_api::DriverDescriptor> {
        let mut descriptors: Vec<_> = self
            .factories
            .values()
            .map(|factory| factory.descriptor())
            .collect();
        descriptors.sort_by_key(|descriptor| descriptor.id);
        descriptors
    }
}

#[cfg(test)]
mod tests {
    use super::DriverRegistry;
    use crate::error::ErrorCategory;

    #[test]
    fn a_driver_left_out_of_the_build_says_how_to_get_it() {
        let registry = DriverRegistry::new();
        let missing = registry.get("duckdb").err().unwrap();
        assert_eq!(missing.category(), ErrorCategory::Capability);
        assert!(
            missing.to_string().contains("--features duckdb"),
            "{missing}"
        );
        let unknown = registry.get("oracle").err().unwrap();
        assert_eq!(unknown.category(), ErrorCategory::Configuration);
        // Postgres is always built in: no feature brings it.
        let unregistered = registry.get("postgres").err().unwrap();
        assert_eq!(unregistered.category(), ErrorCategory::Configuration);
        assert!(!unregistered.to_string().contains("--features"));
    }
}
