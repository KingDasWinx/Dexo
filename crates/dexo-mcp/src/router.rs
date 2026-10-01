use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use dexo_app::mcp::McpConnection;
use dexo_app::{AppError, ErrorCategory};
use dexo_driver_api::Session;
use tokio::sync::{Mutex, MutexGuard};

use crate::backend::McpBackend;
use crate::error::hidden;

/// One connection the profile may use. The session opens on first use and stays open;
/// the mutex serializes calls on it, because a read-only transaction and a write cannot
/// share one server session at the same time.
pub struct ConnectionSlot {
    pub meta: McpConnection,
    session: Mutex<Option<Box<dyn Session>>>,
}

pub struct SessionLease<'a> {
    pub meta: &'a McpConnection,
    guard: MutexGuard<'a, Option<Box<dyn Session>>>,
}

impl SessionLease<'_> {
    pub fn session(&self) -> &dyn Session {
        self.guard
            .as_deref()
            .expect("a lease always holds an open session")
    }

    /// After a network or authentication failure the session is useless; dropping it
    /// makes the next call reconnect instead of failing forever.
    pub fn discard_if_broken(&mut self, error: &AppError) {
        if matches!(
            error.category(),
            ErrorCategory::Network | ErrorCategory::Transport | ErrorCategory::Authentication
        ) {
            *self.guard = None;
        }
    }
}

pub struct McpConnectionRouter {
    slots: BTreeMap<String, ConnectionSlot>,
    backend: Arc<dyn McpBackend>,
    connect_timeout: Duration,
}

impl McpConnectionRouter {
    pub fn new(
        connections: Vec<McpConnection>,
        backend: Arc<dyn McpBackend>,
        connect_timeout: Duration,
    ) -> Self {
        let slots = connections
            .into_iter()
            .map(|meta| {
                (
                    meta.name.clone(),
                    ConnectionSlot {
                        meta,
                        session: Mutex::new(None),
                    },
                )
            })
            .collect();
        Self {
            slots,
            backend,
            connect_timeout,
        }
    }

    pub fn connections(&self) -> impl Iterator<Item = &McpConnection> {
        self.slots.values().map(|slot| &slot.meta)
    }

    pub fn backend(&self) -> &dyn McpBackend {
        self.backend.as_ref()
    }

    /// Explicit routing (MCP-019): a name must be one of the profile's connections, and
    /// leaving it out is only allowed when there is exactly one.
    pub fn resolve(&self, requested: Option<&str>) -> Result<&ConnectionSlot, AppError> {
        match requested {
            Some(name) => self.slots.get(name).ok_or_else(hidden),
            None if self.slots.len() == 1 => Ok(self.slots.values().next().expect("one slot")),
            None => Err(AppError::new(
                ErrorCategory::Configuration,
                "this profile has several connections; pass `connection` (see list_connections)",
            )),
        }
    }

    pub async fn lease<'a>(
        &'a self,
        slot: &'a ConnectionSlot,
    ) -> Result<SessionLease<'a>, AppError> {
        let mut guard = slot.session.lock().await;
        if guard.is_none() {
            let session =
                tokio::time::timeout(self.connect_timeout, self.backend.connect(&slot.meta.name))
                    .await
                    .map_err(|_| {
                        AppError::new(
                            ErrorCategory::Timeout,
                            format!("connecting to {} timed out", slot.meta.name),
                        )
                    })??;
            *guard = Some(session);
        }
        Ok(SessionLease {
            meta: &slot.meta,
            guard,
        })
    }
}
