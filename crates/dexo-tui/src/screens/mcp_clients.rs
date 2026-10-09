//! The agents Dexo can start for -- Claude Code, Codex, Cursor and the rest -- as their own
//! configs say: which profile each starts Dexo with, if any, and whether it is on this
//! machine. Agents' Profiles shows them under the profile they use, and connects one.

use dexo_app::mcp::clients::{ClientState, McpClient};

/// A client as its config says.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientRow {
    pub client: McpClient,
    /// The file it reads its servers from.
    pub path: String,
    /// Where its skill goes, when it has a place for one.
    pub skill: Option<String>,
    /// Its skill file against what Dexo would write for the profile it starts.
    pub skill_state: dexo_app::mcp::clients::SkillState,
    pub state: ClientState,
    /// The agent is on this machine: its command on PATH, or its config's folder there.
    pub found: bool,
}

impl ClientRow {
    /// The profile its config starts Dexo with.
    pub fn profile(&self) -> Option<&str> {
        match &self.state {
            ClientState::SetUp {
                profile: Some(profile),
                ..
            } => Some(profile),
            _ => None,
        }
    }

    /// Where it stands, in a few words: set up and with what, else found here or not.
    pub fn status(&self) -> String {
        match &self.state {
            ClientState::SetUp {
                profile: Some(profile),
                ..
            } => format!("set up · {profile}"),
            ClientState::SetUp { .. } => "set up".into(),
            ClientState::Unusable(_) => "its file cannot be read".into(),
            _ if self.found => "found".into(),
            _ => "not found here".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpClients {
    /// The clients have been read once: an empty list before that is not "none".
    pub loaded: bool,
    pub clients: Vec<ClientRow>,
    /// What a client's entry runs to start Dexo.
    pub command: String,
    /// The folder project files go in: where Dexo was started.
    pub project: String,
}

impl McpClients {
    /// The agents whose config starts Dexo with `profile`.
    pub fn of(&self, profile: &str) -> Vec<&ClientRow> {
        self.clients
            .iter()
            .filter(|row| row.profile() == Some(profile))
            .collect()
    }

    pub fn get(&self, client: McpClient) -> Option<&ClientRow> {
        self.clients.iter().find(|row| row.client == client)
    }
}
