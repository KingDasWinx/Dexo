use std::collections::HashMap;
use std::sync::Arc;

use dexo_driver_api::ConnectionFactory;

use crate::error::{AppError, ErrorCategory};

#[derive(Default)]
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

    pub fn get(&self, driver: &str) -> Result<Arc<dyn ConnectionFactory>, AppError> {
        self.factories.get(driver).cloned().ok_or_else(|| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("unknown driver '{driver}'"),
            )
        })
    }
}
