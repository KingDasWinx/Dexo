use dexo_app::ConnectionProfile;
use dexo_driver_api::TransactionState;

use crate::runtime::SessionId;
use crate::screens::connection::ConnectionForm;
use crate::screens::secret_prompt::DeleteSecretDecision;

#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionRow {
    pub profile: ConnectionProfile,
    pub sessions: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionRow {
    pub id: SessionId,
    pub connection: String,
    pub transaction: TransactionState,
    pub generation: u64,
    pub environment: String,
    pub read_only: bool,
    pub driver: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionIntent {
    Connect,
    Duplicate,
    Test,
    Delete,
    CloseSession,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectionsScreen {
    pub open: bool,
    pub profiles: Vec<ConnectionRow>,
    pub sessions: Vec<SessionRow>,
    pub selected_profile: usize,
    pub selected_session: Option<SessionId>,
    pub form: ConnectionForm,
    pub pending: Option<crate::runtime::OperationId>,
    pub pending_connect: Option<u64>,
    pub delete_target: Option<ConnectionProfile>,
    pub intent: Option<ConnectionIntent>,
    pub error: Option<String>,
}

impl ConnectionsScreen {
    pub fn load_profiles(&mut self, profiles: Vec<ConnectionProfile>) {
        self.profiles = profiles
            .into_iter()
            .map(|profile| {
                let sessions = self
                    .sessions
                    .iter()
                    .filter(|row| row.connection == profile.name)
                    .count();
                ConnectionRow { profile, sessions }
            })
            .collect();
        if self.selected_profile >= self.profiles.len() {
            self.selected_profile = 0;
        }
    }

    pub fn selected(&self) -> Option<&ConnectionProfile> {
        self.profiles
            .get(self.selected_profile)
            .map(|row| &row.profile)
    }

    pub fn session_for(&self, name: &str) -> Option<&SessionRow> {
        self.sessions
            .iter()
            .find(|session| session.connection == name)
    }

    pub fn upsert_session(&mut self, row: SessionRow) {
        // ponytail: one live session per connection name
        self.sessions
            .retain(|item| item.connection != row.connection || item.id == row.id);
        if let Some(existing) = self.sessions.iter_mut().find(|item| item.id == row.id) {
            *existing = row;
        } else {
            self.sessions.push(row);
        }
        self.refresh_session_counts();
    }

    pub fn remove_session(&mut self, id: SessionId) {
        self.sessions.retain(|row| row.id != id);
        if self.selected_session == Some(id) {
            self.selected_session = None;
        }
        self.refresh_session_counts();
    }

    fn refresh_session_counts(&mut self) {
        for row in &mut self.profiles {
            row.sessions = self
                .sessions
                .iter()
                .filter(|session| session.connection == row.profile.name)
                .count();
        }
    }

    pub fn lines(&self, active: Option<SessionId>) -> Vec<String> {
        let mut lines = Vec::new();
        for (index, row) in self.profiles.iter().enumerate() {
            let marker = if index == self.selected_profile {
                ">"
            } else {
                " "
            };
            let group = row.profile.group_path.as_deref().unwrap_or("/");
            let read_only = if row.profile.policy.read_only == Some(true) {
                " ro"
            } else {
                ""
            };
            let session = self.session_for(&row.profile.name);
            let status = match session {
                Some(session) if active == Some(session.id) => "active",
                Some(_) => "connected",
                None => "offline",
            };
            let tx = session
                .map(|session| format!(" {:?}", session.transaction))
                .unwrap_or_default();
            lines.push(format!(
                "{marker} {group} {} [{}] {status}{tx}{read_only}",
                row.profile.name, row.profile.environment
            ));
        }
        if let Some(intent) = self.intent {
            let action = match intent {
                ConnectionIntent::Connect => "connect/switch",
                ConnectionIntent::Duplicate => "duplicate",
                ConnectionIntent::Test => "test",
                ConnectionIntent::Delete => "delete",
                ConnectionIntent::CloseSession => "close",
            };
            lines.push(format!("choose connection to {action}"));
        } else {
            lines.push(
                "Enter connect/switch  c close  t test  e edit  n new  d duplicate  x delete"
                    .into(),
            );
        }
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        if let Some(target) = &self.delete_target {
            lines.push(format!(
                "delete {}? k keep secrets  d delete secrets  esc cancel",
                target.name
            ));
        }
        lines
    }

    pub fn delete_decision(
        &self,
        decision: DeleteSecretDecision,
    ) -> Option<(ConnectionProfile, bool)> {
        self.delete_target
            .clone()
            .map(|profile| (profile, decision == DeleteSecretDecision::DeleteSecrets))
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionsScreen, SessionRow};
    use crate::runtime::SessionId;
    use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
    use dexo_driver_api::TransactionState;

    fn profile(name: &str) -> ConnectionProfile {
        ConnectionProfile::new(
            ConnectionId(uuid::Uuid::nil()),
            None,
            name,
            "postgres",
            "local",
            serde_json::json!({}),
            SecretRef::new("ref".into()),
        )
    }

    #[test]
    fn rows_show_status_and_keep_one_session() {
        let mut screen = ConnectionsScreen::default();
        screen.load_profiles(vec![profile("prod")]);
        let id = SessionId(uuid::Uuid::from_u128(1));
        screen.upsert_session(SessionRow {
            id,
            connection: "prod".into(),
            transaction: TransactionState::Idle,
            generation: 1,
            environment: "local".into(),
            read_only: false,
            driver: "postgres".into(),
        });
        screen.upsert_session(SessionRow {
            id: SessionId(uuid::Uuid::from_u128(2)),
            connection: "prod".into(),
            transaction: TransactionState::Idle,
            generation: 2,
            environment: "local".into(),
            read_only: false,
            driver: "postgres".into(),
        });
        assert_eq!(screen.sessions.len(), 1);
        let dump = screen.lines(Some(screen.sessions[0].id)).join("\n");
        assert!(dump.contains("active"));
        assert!(dump.contains("Enter connect/switch"));
        assert!(!dump.contains("sessions:"));
    }
}
