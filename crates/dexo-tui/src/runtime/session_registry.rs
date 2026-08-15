use std::collections::HashMap;
use std::sync::Arc;

use dexo_app::TransactionService;
use dexo_driver_api::{Session, TransactionMode, TransactionState};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SessionId(pub Uuid);

pub struct ActiveSession {
    pub id: SessionId,
    pub connection: String,
    pub generation: u64,
    pub transaction: TransactionState,
    pub session: Arc<dyn Session>,
}

#[derive(Default)]
pub struct SessionRegistry {
    sessions: HashMap<SessionId, ActiveSession>,
}

impl SessionRegistry {
    pub fn insert(
        &mut self,
        connection: impl Into<String>,
        session: Arc<dyn Session>,
    ) -> SessionId {
        let id = SessionId(Uuid::new_v4());
        self.sessions.insert(
            id,
            ActiveSession {
                id,
                connection: connection.into(),
                generation: 1,
                transaction: TransactionState::Idle,
                session,
            },
        );
        id
    }

    pub fn get(&self, id: SessionId) -> Option<&ActiveSession> {
        self.sessions.get(&id)
    }

    pub fn find_by_connection(&self, connection: &str) -> Option<&ActiveSession> {
        self.sessions
            .values()
            .find(|active| active.connection == connection)
    }

    pub fn set_transaction(
        &mut self,
        id: SessionId,
        state: TransactionState,
    ) -> Result<(), String> {
        let active = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| "session is closed".to_string())?;
        active.transaction = state;
        Ok(())
    }

    pub fn can_reconnect(&self, id: SessionId, read_only: bool) -> Result<(), String> {
        let active = self
            .get(id)
            .ok_or_else(|| "session is closed".to_string())?;
        if !read_only || active.transaction != TransactionState::Idle {
            return Err("unsafe reconnect requires an idle read-only operation".into());
        }
        Ok(())
    }

    pub async fn begin(
        &mut self,
        id: SessionId,
        mode: TransactionMode,
    ) -> Result<TransactionState, String> {
        let session = self.session_arc(id)?;
        TransactionService::begin(session.as_ref(), mode)
            .await
            .map_err(|error| error.to_string())?;
        self.refresh_state(id)
    }

    pub async fn commit(&mut self, id: SessionId) -> Result<TransactionState, String> {
        let session = self.session_arc(id)?;
        TransactionService::commit(session.as_ref())
            .await
            .map_err(|error| error.to_string())?;
        self.refresh_state(id)
    }

    pub async fn rollback(&mut self, id: SessionId) -> Result<TransactionState, String> {
        let session = self.session_arc(id)?;
        TransactionService::rollback(session.as_ref())
            .await
            .map_err(|error| error.to_string())?;
        self.refresh_state(id)
    }

    pub async fn savepoint(&mut self, id: SessionId, name: &str) -> Result<TransactionState, String> {
        let session = self.session_arc(id)?;
        TransactionService::savepoint(session.as_ref(), name)
            .await
            .map_err(|error| error.to_string())?;
        self.refresh_state(id)
    }

    pub async fn rollback_to(
        &mut self,
        id: SessionId,
        name: &str,
    ) -> Result<TransactionState, String> {
        let session = self.session_arc(id)?;
        TransactionService::rollback_to(session.as_ref(), name)
            .await
            .map_err(|error| error.to_string())?;
        self.refresh_state(id)
    }

    pub async fn release_savepoint(
        &mut self,
        id: SessionId,
        name: &str,
    ) -> Result<TransactionState, String> {
        let session = self.session_arc(id)?;
        TransactionService::release_savepoint(session.as_ref(), name)
            .await
            .map_err(|error| error.to_string())?;
        self.refresh_state(id)
    }

    fn session_arc(&self, id: SessionId) -> Result<Arc<dyn Session>, String> {
        self.get(id)
            .map(|active| Arc::clone(&active.session))
            .ok_or_else(|| "session is closed".to_string())
    }

    fn refresh_state(&mut self, id: SessionId) -> Result<TransactionState, String> {
        let active = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| "session is closed".to_string())?;
        let state = active
            .session
            .transactions()
            .map(|control| control.state())
            .unwrap_or(TransactionState::Idle);
        active.transaction = state;
        Ok(state)
    }
}
