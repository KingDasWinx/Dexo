//! Agents' Setup: an agent -- Claude Code, Codex, Cursor and the rest -- pointed at
//! Dexo's MCP server in one go: the profile it uses, made here or picked, and its entry
//! written into the agent's own config. It took six commands at a shell.

use dexo_app::mcp::clients::{ClientState, McpClient};

use crate::widgets::form::FooterFocus;
use crate::widgets::text_input::TextInput;

/// A client as the view lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientRow {
    pub client: McpClient,
    /// The file it reads its servers from.
    pub path: String,
    /// Where its skill goes, when it has a place for one.
    pub skill: Option<String>,
    pub state: ClientState,
    /// The agent is on this machine: its command on PATH, or its config's folder there.
    pub found: bool,
}

impl ClientRow {
    /// What the list says of it: set up and with what, else found here or not.
    pub fn list_status(&self) -> String {
        match &self.state {
            ClientState::SetUp { .. } => self.status(),
            ClientState::Unusable(_) => "unreadable".into(),
            _ if self.found => "found".into(),
            _ => "not found".into(),
        }
    }

    /// What its file says, in words.
    pub fn status(&self) -> String {
        match &self.state {
            ClientState::SetUp {
                profile: Some(profile),
                ..
            } => format!("set up · {profile}"),
            ClientState::SetUp { .. } => "set up".into(),
            ClientState::NoFile | ClientState::NotSetUp => "not set up".into(),
            ClientState::Unusable(_) => "its file cannot be read".into(),
        }
    }
}

/// One row of the form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Row {
    Profile,
    Name,
    Connection(usize),
    Reads,
    Skill,
}

/// The profile a client is set up with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SetupProfile {
    Existing(String),
    New {
        name: String,
        connections: Vec<String>,
        reads: bool,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpSetup {
    pub clients: Vec<ClientRow>,
    /// What a client's entry runs to start Dexo.
    pub command: String,
    /// The folder project files go in: where Dexo was started.
    pub project: String,
    pub selected: usize,
    /// The profile picked: `None` a new one.
    pub profile: Option<String>,
    pub name: TextInput,
    /// The saved connections a new profile can use, and whether it does.
    pub connections: Vec<(String, bool)>,
    /// A new profile may run read-only SQL, besides browsing.
    pub reads: bool,
    pub skill: bool,
    /// The focused row, an index into `rows()`.
    pub row: usize,
    pub footer: FooterFocus,
    pub busy: bool,
    /// What the last Set up said: what it wrote, or why it could not.
    pub outcome: Option<Result<Vec<String>, String>>,
}

impl McpSetup {
    pub fn current(&self) -> Option<&ClientRow> {
        self.clients.get(self.selected)
    }

    pub fn select(&mut self, step: isize) {
        if self.clients.is_empty() {
            return;
        }
        let last = self.clients.len() as isize - 1;
        self.selected = (self.selected as isize + step).clamp(0, last) as usize;
        self.outcome = None;
        self.row = 0;
        self.footer = FooterFocus::Input;
    }

    /// The form's defaults: an existing profile when there is one, else a new one named
    /// `assistant` (or the next free name) on the connection in use.
    pub fn prepare(&mut self, profiles: &[String], connections: Vec<String>, in_use: &str) {
        if self
            .profile
            .as_ref()
            .is_none_or(|name| !profiles.contains(name))
        {
            self.profile = profiles.first().cloned();
        }
        if self.name.as_str().is_empty() || profiles.iter().any(|name| name == self.name.as_str()) {
            let free = std::iter::once("assistant".to_string())
                .chain((2..).map(|n| format!("assistant-{n}")))
                .find(|name| !profiles.contains(name))
                .unwrap_or_default();
            self.name = TextInput::new(free);
        }
        let checked: Vec<String> = self
            .connections
            .iter()
            .filter(|(_, on)| *on)
            .map(|(name, _)| name.clone())
            .collect();
        let fresh = self.connections.is_empty();
        self.connections = connections
            .into_iter()
            .map(|name| {
                let on = if fresh {
                    name == in_use
                } else {
                    checked.contains(&name)
                };
                (name, on)
            })
            .collect();
        if fresh {
            self.reads = true;
            self.skill = true;
        }
        self.row = self.row.min(self.rows().len().saturating_sub(1));
    }

    /// The rows shown: a new profile asks its name, connections and reads.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = vec![Row::Profile];
        if self.profile.is_none() {
            rows.push(Row::Name);
            rows.extend((0..self.connections.len()).map(Row::Connection));
            rows.push(Row::Reads);
        }
        if self.current().is_some_and(|row| row.skill.is_some()) {
            rows.push(Row::Skill);
        }
        rows
    }

    pub fn focused(&self) -> Option<Row> {
        (self.footer == FooterFocus::Input)
            .then(|| self.rows().get(self.row).copied())
            .flatten()
    }

    /// Down a row, then onto the buttons; Up the other way.
    pub fn move_focus(&mut self, step: isize) {
        let count = self.rows().len();
        match (self.footer, step > 0) {
            (FooterFocus::Input, true) if self.row + 1 < count => self.row += 1,
            (FooterFocus::Input, true) => self.footer = FooterFocus::Submit,
            (FooterFocus::Input, false) => self.row = self.row.saturating_sub(1),
            (FooterFocus::Submit, true) => self.footer = FooterFocus::Cancel,
            (FooterFocus::Submit, false) => {
                self.footer = FooterFocus::Input;
                self.row = count.saturating_sub(1);
            }
            (FooterFocus::Cancel, false) => self.footer = FooterFocus::Submit,
            (FooterFocus::Cancel, true) => {}
        }
    }

    /// Left and Right on a row: the next profile, a connection or a switch flipped.
    pub fn change(&mut self, step: isize, profiles: &[String]) {
        match self.focused() {
            Some(Row::Profile) => {
                // `None` (a new one) first, then each profile.
                let choices: Vec<Option<String>> = std::iter::once(None)
                    .chain(profiles.iter().cloned().map(Some))
                    .collect();
                let at = choices
                    .iter()
                    .position(|choice| *choice == self.profile)
                    .unwrap_or(0) as isize;
                let next = (at + step).rem_euclid(choices.len() as isize) as usize;
                self.profile = choices[next].clone();
            }
            Some(Row::Connection(index)) => {
                if let Some((_, on)) = self.connections.get_mut(index) {
                    *on = !*on;
                }
            }
            Some(Row::Reads) => self.reads = !self.reads,
            Some(Row::Skill) => self.skill = !self.skill,
            Some(Row::Name) | None => {}
        }
        self.outcome = None;
    }

    /// What Set up would do, or why it cannot yet.
    pub fn request(&self, profiles: &[String]) -> Result<SetupProfile, String> {
        match &self.profile {
            Some(name) => Ok(SetupProfile::Existing(name.clone())),
            None => {
                let name = self.name.as_str().trim().to_string();
                if name.is_empty()
                    || !name
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
                {
                    return Err(
                        "A profile's name takes letters, digits, - and _: it goes into the agent's config."
                            .into(),
                    );
                }
                if profiles.contains(&name) {
                    return Err(format!("{name} is a profile already: pick it above."));
                }
                let connections: Vec<String> = self
                    .connections
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(name, _)| name.clone())
                    .collect();
                if connections.is_empty() {
                    return Err(if self.connections.is_empty() {
                        "No saved Postgres or MySQL connection to give it: MCP serves those two."
                            .into()
                    } else {
                        "Pick at least one connection for the agent to use.".into()
                    });
                }
                Ok(SetupProfile::New {
                    name,
                    connections,
                    reads: self.reads,
                })
            }
        }
    }

    /// The profile name an entry would run with now.
    pub fn profile_name(&self) -> String {
        self.profile
            .clone()
            .unwrap_or_else(|| self.name.as_str().trim().to_string())
    }
}

/// What a row says about itself while it has the focus.
pub fn row_hint(row: Row) -> &'static str {
    match row {
        Row::Profile => {
            "Left/Right: a new profile, or one already made (its connections and rules stay as they are)"
        }
        Row::Name => "the name the agent's config starts the server with",
        Row::Connection(_) => "Space: whether the agent may use this connection",
        Row::Reads => {
            "yes: read-only SQL too (one SELECT at a time, at most 1000 rows); no: browsing and describing tables only"
        }
        Row::Skill => {
            "a file telling the agent how Dexo's tools behave and to read the notes on tables"
        }
    }
}
