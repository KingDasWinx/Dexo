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
}

#[derive(Clone, Debug, PartialEq)]
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
}

impl Default for ConnectionsScreen {
    fn default() -> Self {
        Self {
            open: false,
            profiles: Vec::new(),
            sessions: Vec::new(),
            selected_profile: 0,
            selected_session: None,
            form: ConnectionForm::default(),
            pending: None,
            pending_connect: None,
            delete_target: None,
        }
    }
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

    pub fn upsert_session(&mut self, row: SessionRow) {
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

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec!["Connections".into()];
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
            lines.push(format!(
                "{marker} {group} {} [{}] {} sessions{read_only}",
                row.profile.name, row.profile.environment, row.sessions
            ));
        }
        if self.sessions.is_empty() {
            lines.push("sessions: none".into());
        } else {
            for session in &self.sessions {
                let marker = if self.selected_session == Some(session.id) {
                    "*"
                } else {
                    " "
                };
                lines.push(format!(
                    "{marker} session {} {:?}",
                    session.connection, session.transaction
                ));
            }
        }
        if let Some(target) = &self.delete_target {
            lines.push(format!(
                "delete {}? k keep secrets  d delete secrets  esc cancel",
                target.name
            ));
        }
        lines
    }

    pub fn delete_decision(&self, decision: DeleteSecretDecision) -> Option<(ConnectionProfile, bool)> {
        self.delete_target
            .clone()
            .map(|profile| (profile, decision == DeleteSecretDecision::DeleteSecrets))
    }
}
