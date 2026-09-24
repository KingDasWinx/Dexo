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
    /// The connection a "Delete connection" dialog is asking about, and which of its
    /// buttons has the focus.
    pub delete_target: Option<ConnectionProfile>,
    pub delete_choice: DeleteChoice,
    pub error: Option<String>,
}

/// The buttons of the "Delete connection" dialog. Cancel comes first in the focus, so
/// an Enter pressed out of habit deletes nothing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DeleteChoice {
    Delete,
    #[default]
    Cancel,
}

impl DeleteChoice {
    pub fn toggle(self) -> Self {
        match self {
            Self::Delete => Self::Cancel,
            Self::Cancel => Self::Delete,
        }
    }
}

/// The key hints under the list. Rendering registers a click target on each, by label,
/// so the line and its targets cannot drift apart.
pub const HINTS: [&str; 7] = [
    "Enter connect",
    "n new",
    "e edit",
    "d duplicate",
    "t test",
    "x delete",
    "c close",
];

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

    /// Opens the "Delete connection" dialog on `profile`, focused on Cancel.
    pub fn ask_delete(&mut self, profile: Option<ConnectionProfile>) {
        self.delete_target = profile;
        self.delete_choice = DeleteChoice::Cancel;
    }

    pub fn lines(&self, active: Option<SessionId>) -> Vec<String> {
        let mut lines = self.profile_lines(active);
        lines.push(String::new());
        lines.extend(self.footer_lines(70));
        lines
    }

    /// The hints, wrapped to `width` columns, and the last error if there is one.
    pub fn footer_lines(&self, width: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        for hint in HINTS {
            match lines.last_mut() {
                Some(line) if line.chars().count() + 2 + hint.chars().count() <= width => {
                    line.push_str("  ");
                    line.push_str(hint);
                }
                _ => lines.push(hint.to_string()),
            }
        }
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines
    }

    pub fn profile_lines(&self, active: Option<SessionId>) -> Vec<String> {
        if self.profiles.is_empty() {
            return vec!["  No connections yet. n adds one.".into()];
        }
        let mut lines = Vec::new();
        for (index, row) in self.profiles.iter().enumerate() {
            let marker = if index == self.selected_profile {
                ">"
            } else {
                " "
            };
            let name = match row
                .profile
                .group_path
                .as_deref()
                .map(str::trim)
                .filter(|group| !group.is_empty())
            {
                Some(group) => format!("{group}/{}", row.profile.name),
                None => row.profile.name.clone(),
            };
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
                "{marker} {name} [{}] {status}{tx}{read_only}",
                row.profile.environment
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
        assert!(dump.contains("> prod [local] active"));
        assert!(!dump.contains("/ prod"));
        assert!(dump.contains("x delete"));
        assert!(!dump.contains("sessions:"));
        for line in dump.lines() {
            assert!(
                line.chars().count() <= 70,
                "connections popup inner width is 70; line too long: {line:?}"
            );
        }
    }

    #[test]
    fn grouped_profiles_show_path_prefix() {
        let mut screen = ConnectionsScreen::default();
        let mut row = profile("db");
        row.group_path = Some("lab/pg".into());
        screen.load_profiles(vec![row]);
        let dump = screen.lines(None).join("\n");
        assert!(dump.contains("> lab/pg/db [local] offline"));
    }
}
