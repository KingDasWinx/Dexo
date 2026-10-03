use std::collections::{BTreeMap, BTreeSet};

use dexo_app::{ConnectionProfile, Environment};
use dexo_driver_api::TransactionState;

use crate::runtime::SessionId;
use crate::screen::widgets::{FieldRow, Search};
use crate::screens::connection::ConnectionForm;
use crate::screens::secret_prompt::DeleteSecretDecision;
use crate::theme::Role;

#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionRow {
    pub profile: ConnectionProfile,
    pub sessions: usize,
    /// Opened from a URL or as the demo, and not saved.
    pub temporary: bool,
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
    /// The screen the connection form was opened from, to go back to once it closes:
    /// `n` in the sidebar adds a connection and lands back in the sidebar.
    pub form_from: Option<crate::model::Screen>,
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
    /// Connections this session opened without saving (`dexo <url>`, `dexo --demo`).
    /// They are kept here, apart from the saved list, so reloading that list from
    /// storage does not drop them while their session is still open.
    pub temporary: Vec<ConnectionProfile>,
    /// Databases running in Docker, listed after the saved connections; a selection
    /// past the saved ones is one of these.
    pub docker: Vec<dexo_app::docker::DockerDatabase>,
    /// The list's search, over name, host, database, group and driver.
    pub search: Search,
    /// Only the connections with a session open.
    pub connected_only: bool,
    /// Only the connections of one environment.
    pub env: Option<Environment>,
    /// The groups folded under their heading.
    pub folded: BTreeSet<String>,
    /// The group heading the pick is on, rather than a row.
    pub picked_group: Option<String>,
}

/// One row of the list a pick can be on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Item {
    /// A group's heading, with how many of its connections pass the filters.
    Group {
        name: String,
        count: usize,
        folded: bool,
    },
    /// A saved connection, or past them a database found in Docker: what
    /// `selected_profile` holds.
    Row(usize),
}

/// The environments `v` walks through after all of them.
pub const ENVIRONMENTS: [Environment; 4] = [
    Environment::Production,
    Environment::Staging,
    Environment::Development,
    Environment::Local,
];

/// How an environment is said in a chip or a column.
pub fn env_name(environment: Environment) -> &'static str {
    match environment {
        Environment::Production => "prod",
        Environment::Staging => "staging",
        Environment::Development => "dev",
        Environment::Local => "local",
    }
}

/// The group a connection is filed under, if any.
fn group_of(profile: &ConnectionProfile) -> Option<&str> {
    profile
        .group_path
        .as_deref()
        .map(str::trim)
        .filter(|group| !group.is_empty())
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

impl ConnectionsScreen {
    pub fn load_profiles(&mut self, profiles: Vec<ConnectionProfile>) {
        // `ProfileSaved` hands the current rows back, temporary ones included.
        let picked = self.selected().map(|profile| profile.id);
        let mut saved: Vec<ConnectionProfile> = profiles
            .into_iter()
            .filter(|profile| self.temporary.iter().all(|other| other.id != profile.id))
            .collect();
        // The order a restart gives, grouped and by name: a connection added or renamed
        // went to the end of the list until then.
        saved.sort_by(|a, b| {
            (a.group_path.as_deref().unwrap_or(""), a.name.as_str())
                .cmp(&(b.group_path.as_deref().unwrap_or(""), b.name.as_str()))
        });
        let rows: Vec<(ConnectionProfile, bool)> = saved
            .into_iter()
            .map(|profile| (profile, false))
            .chain(
                self.temporary
                    .iter()
                    .cloned()
                    .map(|profile| (profile, true)),
            )
            .collect();
        self.profiles = rows
            .into_iter()
            .map(|(profile, temporary)| {
                let sessions = self
                    .sessions
                    .iter()
                    .filter(|row| row.connection == profile.name)
                    .count();
                ConnectionRow {
                    profile,
                    sessions,
                    temporary,
                }
            })
            .collect();
        // The pick stays on its connection, wherever the new order put it.
        if let Some(index) =
            picked.and_then(|id| self.profiles.iter().position(|row| row.profile.id == id))
        {
            self.selected_profile = index;
        } else if self.selected_profile >= self.profiles.len() {
            self.selected_profile = 0;
        }
    }

    /// The Docker database the selection is on, when it is past the saved connections.
    pub fn selected_docker(&self) -> Option<&dexo_app::docker::DockerDatabase> {
        self.selected_profile
            .checked_sub(self.profiles.len())
            .and_then(|index| self.unsaved_docker().nth(index))
    }

    /// Saved connections, then the Docker ones: what Up and Down walk.
    pub fn row_count(&self) -> usize {
        self.profiles.len() + self.unsaved_docker().count()
    }

    /// The Docker databases no saved connection dials already -- same driver, host and
    /// port.
    pub fn unsaved_docker(&self) -> impl Iterator<Item = &dexo_app::docker::DockerDatabase> {
        self.docker.iter().filter(|database| {
            let connection = &database.connection;
            !self.profiles.iter().any(|row| {
                let config = &row.profile.config;
                let port = config.get("port").and_then(|port| {
                    port.as_u64()
                        .or_else(|| port.as_str().and_then(|port| port.parse().ok()))
                });
                row.profile.driver == connection.driver
                    && config.get("host").and_then(serde_json::Value::as_str)
                        == Some(connection.host.as_str())
                    && port == connection.port.map(u64::from)
            })
        })
    }

    pub fn is_temporary(&self, name: &str) -> bool {
        self.temporary.iter().any(|profile| profile.name == name)
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

    /// The picked row in full, as fields: where a saved connection goes and under which
    /// rules, or what a database found in Docker would be added as.
    pub fn detail_fields(&self, active: Option<SessionId>) -> Vec<FieldRow> {
        if let Some(database) = self.selected_docker() {
            let connection = &database.connection;
            let mut rows = vec![
                FieldRow::Field("Container", database.container.clone()),
                FieldRow::Field("Driver", driver_name(&connection.driver)),
                FieldRow::Field(
                    "Address",
                    format!(
                        "{}:{}",
                        connection.host,
                        connection.port.unwrap_or_default()
                    ),
                ),
            ];
            push_set(&mut rows, "Database", &connection.database);
            push_set(&mut rows, "User", &connection.username);
            rows.push(FieldRow::Blank);
            rows.push(FieldRow::Text(
                "Enter fills a new connection from it; nothing is saved until it is.".into(),
            ));
            return rows;
        }
        let Some(row) = self.profiles.get(self.selected_profile) else {
            return Vec::new();
        };
        let profile = &row.profile;
        let config = &profile.config;
        let text = |key: &str| config_text(config, key);
        let mut rows = vec![FieldRow::Field("Driver", driver_name(&profile.driver))];
        rows.push(FieldRow::Field(
            "Environment",
            if profile.environment.is_empty() {
                "local".into()
            } else {
                profile.environment.clone()
            },
        ));
        let path = text("path");
        if path.is_empty() {
            let port = text("port");
            let host = text("host");
            rows.push(FieldRow::Field(
                "Address",
                if port.is_empty() {
                    host
                } else {
                    format!("{host}:{port}")
                },
            ));
            push_set(&mut rows, "Database", &text("database"));
            push_set(&mut rows, "User", &text("username"));
            let password_command = text("password_command");
            rows.push(FieldRow::Field(
                "Password",
                if password_command.is_empty() {
                    "in the keychain".into()
                } else {
                    format!("from `{}`", shown_path(&password_command, 40))
                },
            ));
        } else {
            rows.push(FieldRow::Field("File", shown_path(&path, 60)));
        }
        if let Some(group) = group_of(profile) {
            rows.push(FieldRow::Field("Group", group.to_string()));
        }
        if row.temporary {
            rows.push(FieldRow::Field(
                "Saved",
                "no, open for this session only".into(),
            ));
        }
        let pre_connect = text("pre_connect");
        if !pre_connect.is_empty() {
            rows.push(FieldRow::Field("Runs first", format!("`{pre_connect}`")));
        }
        let mut network = Vec::new();
        if let Some(tls) = config.get("tls").and_then(|tls| tls.get("mode")) {
            network.push(FieldRow::Field(
                "TLS",
                tls.as_str().unwrap_or("-").to_string(),
            ));
        }
        if let Some(ssh) = config.get("ssh") {
            network.push(FieldRow::Field(
                "SSH",
                format!(
                    "{}@{}:{}",
                    config_text(ssh, "username"),
                    config_text(ssh, "host"),
                    config_text(ssh, "port")
                ),
            ));
        }
        if let Some(proxy) = config.get("proxy") {
            let kind = config_text(proxy, "kind");
            network.push(FieldRow::Field(
                "Proxy",
                format!(
                    "{} {}:{}",
                    if kind.is_empty() {
                        "http"
                    } else {
                        kind.as_str()
                    },
                    config_text(proxy, "host"),
                    config_text(proxy, "port")
                ),
            ));
        }
        if !network.is_empty() {
            rows.push(FieldRow::Section("Network"));
            rows.extend(network);
        }
        let policy = &profile.policy;
        let mut rules = Vec::new();
        if policy.read_only == Some(true) {
            rules.push(FieldRow::Field("Read-only", "yes".into()));
        }
        if policy.confirm_destructive == Some(true) {
            rules.push(FieldRow::Field("Confirms", "destructive statements".into()));
        }
        if policy.require_verified_tls == Some(true) {
            rules.push(FieldRow::Field("TLS", "verified only".into()));
        }
        if let Some(max_rows) = policy.max_rows {
            rules.push(FieldRow::Field("Max rows", max_rows.to_string()));
        }
        if let Some(secs) = policy.timeout_secs {
            rules.push(FieldRow::Field("Timeout", format!("{secs}s")));
        }
        if !rules.is_empty() {
            rows.push(FieldRow::Section("Rules"));
            rows.extend(rules);
        }
        if let Some(session) = self.session_for(&profile.name) {
            rows.push(FieldRow::Section("Session"));
            rows.push(FieldRow::Field(
                "State",
                if active == Some(session.id) {
                    "in use".into()
                } else {
                    "connected".into()
                },
            ));
            rows.push(FieldRow::Field(
                "Transaction",
                match session.transaction {
                    TransactionState::Active => "open",
                    TransactionState::Failed => "failed",
                    TransactionState::Unknown => "unknown",
                    _ => "none open",
                }
                .into(),
            ));
        }
        rows
    }

    /// The list as drawn, filtered and in order: the connections in no group, then each
    /// group under its heading, then the databases found in Docker -- those only while no
    /// filter but the search is on, as they have no session and no environment.
    pub fn items(&self) -> Vec<Item> {
        let mut ungrouped = Vec::new();
        let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (index, row) in self.profiles.iter().enumerate() {
            if !self.shows(row) {
                continue;
            }
            match group_of(&row.profile) {
                Some(group) => groups.entry(group).or_default().push(index),
                None => ungrouped.push(index),
            }
        }
        let mut items: Vec<Item> = ungrouped.into_iter().map(Item::Row).collect();
        // A search opens the folded groups: what it found is shown.
        let searching = !self.search.input.trim().is_empty();
        for (name, rows) in groups {
            let folded = !searching && self.folded.contains(name);
            items.push(Item::Group {
                name: name.to_string(),
                count: rows.len(),
                folded,
            });
            if !folded {
                items.extend(rows.into_iter().map(Item::Row));
            }
        }
        if !self.connected_only && self.env.is_none() {
            let saved = self.profiles.len();
            items.extend(
                self.unsaved_docker()
                    .enumerate()
                    .filter(|(_, database)| {
                        let connection = &database.connection;
                        self.search.matches([
                            database.container.as_str(),
                            connection.driver.as_str(),
                            connection.host.as_str(),
                            connection.database.as_str(),
                        ])
                    })
                    .map(|(offset, _)| Item::Row(saved + offset)),
            );
        }
        items
    }

    /// Whether the search and the filters let `row` through.
    fn shows(&self, row: &ConnectionRow) -> bool {
        let profile = &row.profile;
        if self.connected_only && self.session_for(&profile.name).is_none() {
            return false;
        }
        if self
            .env
            .is_some_and(|env| Environment::parse_strict(&profile.environment) != env)
        {
            return false;
        }
        let text = |key: &str| {
            profile
                .config
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        };
        let driver = dexo_driver_api::DriverDescriptor::for_id(&profile.driver)
            .map(|descriptor| descriptor.display_name)
            .unwrap_or_default();
        self.search.matches([
            profile.name.as_str(),
            text("host"),
            text("database"),
            text("path"),
            group_of(profile).unwrap_or_default(),
            profile.driver.as_str(),
            driver,
        ])
    }

    /// Whether a filter is on: the search or one of the chips.
    pub fn filtered(&self) -> bool {
        !self.search.input.is_empty() || self.connected_only || self.env.is_some()
    }

    /// The search and the filters off.
    pub fn clear_filters(&mut self) {
        self.search.input.clear();
        self.search.typing = false;
        self.connected_only = false;
        self.env = None;
        self.keep_pick_shown();
    }

    /// Where the pick is among `items`.
    pub fn cursor(&self, items: &[Item]) -> Option<usize> {
        items
            .iter()
            .position(|item| match (item, &self.picked_group) {
                (Item::Group { name, .. }, Some(picked)) => name == picked,
                (Item::Row(index), None) => *index == self.selected_profile,
                _ => false,
            })
    }

    fn pick_item(&mut self, item: &Item) {
        match item {
            Item::Group { name, .. } => self.picked_group = Some(name.clone()),
            Item::Row(index) => self.pick(*index),
        }
    }

    /// The pick on row `index` of the saved connections and the Docker ones.
    pub fn pick(&mut self, index: usize) {
        self.picked_group = None;
        self.selected_profile = index;
    }

    /// Moves the pick `delta` rows of the list, staying in it.
    pub fn step(&mut self, delta: isize) {
        let items = self.items();
        let Some(last) = items.len().checked_sub(1) else {
            return;
        };
        let at = match self.cursor(&items) {
            Some(at) => at.saturating_add_signed(delta).min(last),
            None => 0,
        };
        self.pick_item(&items[at]);
    }

    /// The pick on the first row, or the last.
    pub fn pick_end(&mut self, last: bool) {
        let items = self.items();
        let item = if last { items.last() } else { items.first() };
        if let Some(item) = item {
            self.pick_item(item);
        }
    }

    /// The pick back on the list when a filter hid it: on its first row.
    pub fn keep_pick_shown(&mut self) {
        let items = self.items();
        if self.cursor(&items).is_none()
            && let Some(first) = items.first()
        {
            self.pick_item(first);
        }
    }

    /// The saved connection the pick is on, when the list shows it.
    pub fn picked(&self) -> Option<&ConnectionProfile> {
        self.picked_row()
            .and_then(|index| self.profiles.get(index))
            .map(|row| &row.profile)
    }

    /// The database found in Docker the pick is on, when the list shows it.
    pub fn picked_docker(&self) -> Option<&dexo_app::docker::DockerDatabase> {
        self.picked_row().and_then(|_| self.selected_docker())
    }

    fn picked_row(&self) -> Option<usize> {
        (self.picked_group.is_none() && self.items().contains(&Item::Row(self.selected_profile)))
            .then_some(self.selected_profile)
    }

    /// Folds or unfolds the group the pick is on or in, and puts the pick on its heading.
    pub fn fold(&mut self, fold: bool) {
        let Some(group) = self
            .picked_group
            .clone()
            .or_else(|| self.picked().and_then(group_of).map(str::to_string))
        else {
            return;
        };
        if fold {
            self.folded.insert(group.clone());
        } else {
            self.folded.remove(&group);
        }
        self.picked_group = Some(group);
    }

    /// A saved connection's row, column by column.
    pub fn cells(&self, index: usize, active: Option<SessionId>) -> Option<Cells> {
        let row = self.profiles.get(index)?;
        let profile = &row.profile;
        let session = self.session_for(&profile.name);
        let state = match session {
            Some(session) if active == Some(session.id) => State::InUse,
            Some(_) => State::Connected,
            None => State::Offline,
        };
        // Idle is the null state, and `Idle` was a Rust name beside a status.
        let tx = match session.map(|session| session.transaction) {
            Some(TransactionState::Active) => " (tx open)",
            Some(TransactionState::Failed) => " (tx failed)",
            Some(TransactionState::Unknown) => " (tx unknown)",
            _ => "",
        };
        let read_only = if profile.policy.read_only == Some(true) {
            " ro"
        } else {
            ""
        };
        let (env, env_role) = env_word(&profile.environment);
        Some(Cells {
            state,
            name: format!("{}{read_only}{tx}", profile.name),
            driver: driver_name(&profile.driver),
            env,
            env_role,
            address: address(&profile.config),
        })
    }

    /// The containers found in Docker that a saved connection already dials: not offered
    /// again, but named, or a user looking for theirs thinks Dexo cannot see it.
    pub fn saved_docker(&self) -> Vec<&str> {
        self.docker
            .iter()
            .filter(|database| {
                !self
                    .unsaved_docker()
                    .any(|shown| std::ptr::eq(shown, *database))
            })
            .map(|database| database.container.as_str())
            .collect()
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

/// Where a connection stands: in use, connected, or not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    InUse,
    Connected,
    Offline,
}

/// A saved connection's row, column by column.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cells {
    pub state: State,
    pub name: String,
    pub driver: String,
    pub env: String,
    pub env_role: Role,
    pub address: String,
}

/// An environment as a short word coloured by its risk. A label Dexo does not know is
/// shown as it is, coloured as production: the guards read it so too.
pub fn env_word(environment: &str) -> (String, Role) {
    match Environment::known(environment) {
        Some(environment) => (
            env_name(environment).to_string(),
            match environment {
                Environment::Production => Role::Production,
                Environment::Staging => Role::Staging,
                Environment::Development => Role::Development,
                Environment::Local => Role::Muted,
            },
        ),
        None if environment.is_empty() => ("local".into(), Role::Muted),
        None => (environment.to_string(), Role::Production),
    }
}

/// A driver as people name it: `PostgreSQL` for `postgres`.
pub fn driver_name(driver: &str) -> String {
    dexo_driver_api::DriverDescriptor::for_id(driver)
        .map(|descriptor| descriptor.display_name.to_string())
        .unwrap_or_else(|| driver.to_string())
}

/// Where a connection points: `host[:port]/database`, or its file from `~`.
pub fn address(config: &serde_json::Value) -> String {
    let path = config_text(config, "path");
    if !path.is_empty() {
        return shown_path(&path, usize::MAX);
    }
    let mut address = config_text(config, "host");
    let port = config_text(config, "port");
    if !port.is_empty() {
        address.push(':');
        address.push_str(&port);
    }
    let database = config_text(config, "database");
    if !database.is_empty() {
        address.push('/');
        address.push_str(&database);
    }
    address
}

/// A config value as text, a number included; empty when unset.
fn config_text(config: &serde_json::Value, key: &str) -> String {
    config
        .get(key)
        .map(|value| match value {
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

/// A path from `~` when under the home folder, its middle cut past `width` columns.
fn shown_path(path: &str, width: usize) -> String {
    let path = match std::env::home_dir() {
        Some(home) => crate::runtime::home_relative(std::path::Path::new(path), &home),
        None => path.to_string(),
    };
    crate::widgets::document_tabs::cut_middle(&path, width)
}

fn push_set(rows: &mut Vec<FieldRow>, label: &'static str, value: &str) {
    if !value.is_empty() {
        rows.push(FieldRow::Field(label, value.to_string()));
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
        let cells = screen.cells(0, Some(screen.sessions[0].id)).unwrap();
        assert_eq!(cells.state, super::State::InUse);
        assert_eq!((cells.name.as_str(), cells.env.as_str()), ("prod", "local"));
    }

    #[test]
    fn grouped_profiles_are_listed_under_their_group() {
        let mut screen = ConnectionsScreen::default();
        let mut row = profile("db");
        row.group_path = Some("lab/pg".into());
        screen.load_profiles(vec![row]);
        assert_eq!(
            screen.items(),
            [
                super::Item::Group {
                    name: "lab/pg".into(),
                    count: 1,
                    folded: false
                },
                super::Item::Row(0)
            ]
        );
    }
}
