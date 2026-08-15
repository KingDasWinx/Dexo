use std::collections::BTreeMap;
use std::sync::Arc;

use dexo_driver_api::Session;
use rmcp::ErrorData as McpError;

use crate::error::hidden_error;

pub struct McpSessionSlot {
    pub session: Arc<dyn Session>,
}

pub struct McpConnectionRouter {
    allowed: BTreeMap<String, McpSessionSlot>,
}

impl McpConnectionRouter {
    pub fn new(allowed: BTreeMap<String, McpSessionSlot>) -> Self {
        Self { allowed }
    }

    pub async fn session(&self, id: &str) -> Result<Arc<dyn Session>, McpError> {
        self.allowed
            .get(id)
            .map(|slot| Arc::clone(&slot.session))
            .ok_or_else(|| McpError::invalid_params(hidden_error(), None))
    }

    pub fn resolve<'a>(&'a self, requested: Option<&'a str>) -> Result<&'a str, McpError> {
        match requested {
            Some(id) if self.allowed.contains_key(id) => Ok(id),
            Some(_) => Err(McpError::invalid_params(hidden_error(), None)),
            None if self.allowed.len() == 1 => Ok(self.allowed.keys().next().unwrap().as_str()),
            None => Err(McpError::invalid_params(hidden_error(), None)),
        }
    }
}
