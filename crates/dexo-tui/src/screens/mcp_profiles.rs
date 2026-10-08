//! Agents' Profiles: each profile an agent connects through -- what it reads and sees,
//! what it may write on each of its connections, which agents start Dexo with it -- and
//! the forms that change them: New profile, Edit, Writes and Connect agent, each in the
//! form every form follows (`docs/design.md`).

use crossterm::event::{KeyCode, KeyEvent};
use dexo_app::mcp::WriteKind;
use dexo_app::mcp::clients::McpClient;

use crate::screens::mcp_clients::McpClients;
use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

pub use dexo_app::mcp::grant::duration_words;

/// A kind of write as the form's box says it, and what it covers.
pub fn kind_label(kind: WriteKind) -> (&'static str, &'static str) {
    match kind {
        WriteKind::Rows => ("change rows", "insert, update, delete, and SQL that writes"),
        WriteKind::Schema => ("change schema", "CREATE, ALTER, DROP, indexes, views"),
        WriteKind::Sessions => ("end sessions", "cancel a query, end a server session"),
    }
}

fn kinds_words(kinds: &[WriteKind]) -> String {
    kinds
        .iter()
        .map(|kind| kind.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a profile may write on one connection, as the ledger has it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Access {
    pub kinds: Vec<WriteKind>,
    pub expires_in_secs: i64,
    pub until_revoked: bool,
}

impl Access {
    /// How long is left, as the end of a line: `28 min left`, `until revoked`.
    pub fn left(&self) -> String {
        if self.until_revoked {
            "until revoked".into()
        } else {
            format!("{} left", duration_words(self.expires_in_secs))
        }
    }

    /// `rows, schema · 28 min left`.
    pub fn words(&self) -> String {
        format!("{} · {}", kinds_words(&self.kinds), self.left())
    }
}

/// One connection a profile may use, with what it may write there.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectionLine {
    pub name: String,
    pub environment: String,
    /// Why MCP never writes to it -- production, or kept read-only -- which no access
    /// changes.
    pub refused: Option<String>,
    /// A PostgreSQL connection: its tables are `database.schema.table`.
    pub postgres: bool,
    pub access: Option<Access>,
}

/// What a profile sees: every object of its connections, all of them but some, or only
/// some.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Sees {
    #[default]
    All,
    Except,
    Only,
}

/// A profile's selectors read as what it sees: the shape, the selectors the boxes stand
/// for, and -- seeing only some -- the deny rules kept as they are.
pub fn sees_of(scopes: &[String]) -> (Sees, Vec<String>, Vec<String>) {
    let (mut allows, mut denies) = (Vec::new(), Vec::new());
    for scope in scopes {
        match scope.split_once(' ') {
            Some(("deny", selector)) => denies.push(selector.to_string()),
            Some((_, selector)) => allows.push(selector.to_string()),
            None => allows.push(scope.clone()),
        }
    }
    if allows == ["*"] {
        if denies.is_empty() {
            (Sees::All, Vec::new(), Vec::new())
        } else {
            (Sees::Except, denies, Vec::new())
        }
    } else {
        (Sees::Only, allows, denies)
    }
}

/// What a profile sees, in words.
pub fn sees_words(sees: Sees, checked: &[String], kept: &[String]) -> String {
    match sees {
        Sees::All => "everything in its connections".into(),
        Sees::Except => format!("everything, except {}", checked.join(", ")),
        Sees::Only if checked.is_empty() => "nothing yet".into(),
        Sees::Only if kept.is_empty() => format!("only {}", checked.join(", ")),
        Sees::Only => format!("only {}, never {}", checked.join(", "), kept.join(", ")),
    }
}

/// A profile as Agents' Profiles shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpProfileSummary {
    pub name: String,
    pub enabled: bool,
    /// The profile offers `query_execute_read`, a raw read-only SQL statement.
    pub raw_read: bool,
    /// Its selectors, as `allow …` and `deny …`.
    pub scopes: Vec<String>,
    /// It lists no connection, so it may use any: `connections` are all MCP serves.
    pub any: bool,
    pub connections: Vec<ConnectionLine>,
    /// The kinds a write access can give it: each with a tool it does not deny.
    pub kinds: Vec<WriteKind>,
}

impl McpProfileSummary {
    pub fn reads_words(&self) -> &'static str {
        if self.raw_read {
            "tables and SELECT, 1000 rows at most"
        } else {
            "tables only: browse, describe, read pages"
        }
    }

    pub fn sees_words(&self) -> String {
        let (sees, checked, kept) = sees_of(&self.scopes);
        sees_words(sees, &checked, &kept)
    }

    /// How many of its connections it may write to now.
    pub fn writes(&self) -> usize {
        self.connections
            .iter()
            .filter(|connection| connection.access.is_some())
            .count()
    }
}

/// A row of the detail the keys walk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Row {
    /// One of the profile's connections, by its place.
    Connection(usize),
    Agent(McpClient),
}

/// What a person is asked to confirm on Profiles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum McpConfirmKind {
    Enable(String),
    RevokeAll { profiles: usize, grants: usize },
    DeleteProfile(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpConfirm {
    pub kind: McpConfirmKind,
    pub focus: FooterFocus,
}

impl McpConfirm {
    /// Cancel has the focus, so an Enter out of habit changes nothing.
    pub fn new(kind: McpConfirmKind) -> Self {
        Self {
            kind,
            focus: FooterFocus::Cancel,
        }
    }

    pub fn submit_label(&self) -> &'static str {
        match self.kind {
            McpConfirmKind::Enable(_) => "Enable",
            McpConfirmKind::RevokeAll { .. } => "Stop all writes",
            McpConfirmKind::DeleteProfile(_) => "Delete",
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut lines = match &self.kind {
            McpConfirmKind::Enable(name) => vec![
                format!("Enable {name}?"),
                "Agents that start Dexo with it can then read what it allows.".into(),
            ],
            McpConfirmKind::RevokeAll { profiles, grants } => vec![
                format!(
                    "Stop every agent write: {} in {}?",
                    plural(*grants, "connection", "connections"),
                    plural(*profiles, "profile", "profiles")
                ),
                "Agents lose them at once; a write waiting for approval is refused.".into(),
            ],
            McpConfirmKind::DeleteProfile(name) => vec![
                format!("Delete the profile {name}?"),
                "Its writes go with it, and agents using it stop working.".into(),
            ],
        };
        lines.push(String::new());
        lines.push(footer_line(self.submit_label(), self.focus));
        lines
    }
}

/// What was asked for before the profiles were read, done once they are.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WhenLoaded {
    Writes,
    Agent,
    RevokeAll,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    Profile(ProfileForm),
    Writes(WritesForm),
    Agent(AgentForm),
}

impl Form {
    pub fn busy(&mut self) -> &mut bool {
        match self {
            Self::Profile(form) => &mut form.busy,
            Self::Writes(form) => &mut form.busy,
            Self::Agent(form) => &mut form.busy,
        }
    }

    /// A refusal from where it was saved, on the row it is about.
    pub fn fail(&mut self, message: String) {
        *self.busy() = false;
        match self {
            Self::Profile(form) => form.fail(message),
            Self::Writes(form) => form.fail(message),
            Self::Agent(form) => form.error = Some(message),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpProfilesScreen {
    pub profiles: Vec<McpProfileSummary>,
    pub selected: usize,
    /// The detail's row the keys walk: its connections, then its agents.
    pub row: usize,
    /// What the last action did, until the pick moves.
    pub status: String,
    pub confirm: Option<McpConfirm>,
    pub form: Option<Form>,
    pub when_loaded: Option<WhenLoaded>,
    /// The profile to pick once they are read again: one just made.
    pub pick: Option<String>,
    /// They were read at least once: what was asked before then can be done.
    pub loaded: bool,
}

impl McpProfilesScreen {
    /// A profile to draw: `assistant`, disabled, seeing all but one table of `local`,
    /// where it may change rows for 15 minutes.
    pub fn fixture() -> Self {
        Self {
            profiles: vec![McpProfileSummary {
                name: "assistant".into(),
                enabled: false,
                raw_read: false,
                scopes: vec!["allow *".into(), "deny db.public.secrets".into()],
                any: false,
                connections: vec![ConnectionLine {
                    name: "local".into(),
                    environment: "local".into(),
                    refused: None,
                    postgres: true,
                    access: Some(Access {
                        kinds: vec![WriteKind::Rows],
                        expires_in_secs: 900,
                        until_revoked: false,
                    }),
                }],
                kinds: WriteKind::ALL.to_vec(),
            }],
            ..Self::default()
        }
    }

    pub fn selected_profile(&self) -> Option<&McpProfileSummary> {
        self.profiles.get(self.selected)
    }

    pub fn name(&self) -> Option<String> {
        self.selected_profile().map(|profile| profile.name.clone())
    }

    /// The rows of the picked profile's detail, in order: its connections, then the
    /// agents that start Dexo with it.
    pub fn rows(&self, clients: &McpClients) -> Vec<Row> {
        let Some(profile) = self.selected_profile() else {
            return Vec::new();
        };
        (0..profile.connections.len())
            .map(Row::Connection)
            .chain(
                clients
                    .of(&profile.name)
                    .into_iter()
                    .map(|row| Row::Agent(row.client)),
            )
            .collect()
    }

    pub fn current(&self, clients: &McpClients) -> Option<Row> {
        self.rows(clients).get(self.row).copied()
    }

    /// The walked connection.
    pub fn walked_connection(&self, clients: &McpClients) -> Option<&ConnectionLine> {
        match self.current(clients)? {
            Row::Connection(at) => self.selected_profile()?.connections.get(at),
            Row::Agent(_) => None,
        }
    }

    pub fn walk(&mut self, delta: isize, clients: &McpClients) {
        let last = self.rows(clients).len().saturating_sub(1);
        self.row = self.row.saturating_add_signed(delta).min(last);
    }

    pub fn select_index(&mut self, index: usize) {
        let moved = index.min(self.profiles.len().saturating_sub(1));
        if moved != self.selected {
            // What the last action said was about the profile it was done to.
            self.status.clear();
            self.row = 0;
        }
        self.selected = moved;
    }

    pub fn select(&mut self, delta: isize) {
        self.select_index(self.selected.saturating_add_signed(delta));
    }

    /// Takes the profiles as read again. The pick stays on its profile by name, so what
    /// was just made -- a profile, a write access -- is still on screen.
    pub fn load_profiles(&mut self, profiles: Vec<McpProfileSummary>) {
        let keep = self.pick.take().or_else(|| self.name());
        self.profiles = profiles;
        self.loaded = true;
        self.selected = keep
            .and_then(|keep| self.profiles.iter().position(|p| p.name == keep))
            .unwrap_or_else(|| self.selected.min(self.profiles.len().saturating_sub(1)));
        // A confirmation belongs to what it was asked about.
        let applies = match self.confirm.as_ref().map(|confirm| &confirm.kind) {
            Some(McpConfirmKind::Enable(name) | McpConfirmKind::DeleteProfile(name)) => {
                self.profiles.iter().any(|profile| &profile.name == name)
            }
            Some(McpConfirmKind::RevokeAll { .. }) => {
                self.profiles.iter().any(|profile| profile.writes() > 0)
            }
            None => true,
        };
        if !applies {
            self.confirm = None;
        }
    }

    /// Flips the picked profile. Enabling lets agents read a database, so it asks first;
    /// disabling only takes that away and does not. The new state once it changed.
    pub fn toggle_selected(&mut self) -> Option<bool> {
        let profile = self.selected_profile()?;
        if profile.enabled {
            self.status = format!("Disabled {}: agents can no longer connect.", profile.name);
            self.profiles[self.selected].enabled = false;
            return Some(false);
        }
        self.confirm = Some(McpConfirm::new(McpConfirmKind::Enable(
            profile.name.clone(),
        )));
        None
    }

    /// Asks to stop every write of every profile, when there are any.
    pub fn ask_revoke_all(&mut self) {
        let grants: usize = self.profiles.iter().map(McpProfileSummary::writes).sum();
        if grants == 0 {
            self.status = "No agent may write: there is nothing to stop.".into();
            return;
        }
        let profiles = self.profiles.iter().filter(|p| p.writes() > 0).count();
        self.confirm = Some(McpConfirm::new(McpConfirmKind::RevokeAll {
            profiles,
            grants,
        }));
    }

    pub fn ask_delete(&mut self) {
        if let Some(name) = self.name() {
            self.confirm = Some(McpConfirm::new(McpConfirmKind::DeleteProfile(name)));
        }
    }

    /// What the screen says with no profile to show.
    pub const EMPTY: [&'static str; 2] = [
        "No profile yet.",
        "An agent connects through a profile, which says what it may read.",
    ];
}

/// What a key did to a form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormKey {
    Stay,
    Submit,
    Cancel,
}

/// The row `delta` places from `at`, wrapping.
fn walk<R: Copy + PartialEq>(rows: &[R], at: R, delta: isize) -> R {
    let index = rows.iter().position(|row| *row == at).unwrap_or(0) as isize;
    rows[(index + delta).rem_euclid(rows.len().max(1) as isize) as usize]
}

/// The arrows on a grid of `sizes` boxes, two to a row: true while they move inside it.
/// Up past the first row and Down past the last leave it.
fn grid_key(sizes: &[usize], cursor: &mut usize, code: KeyCode) -> bool {
    let rows = crate::widgets::panel::grid_rows(sizes);
    let last = sizes.iter().sum::<usize>().saturating_sub(1);
    let at = (*cursor).min(last);
    let moved = match code {
        KeyCode::Down => crate::widgets::panel::grid_step(&rows, at, false),
        KeyCode::Up => crate::widgets::panel::grid_step(&rows, at, true),
        KeyCode::Left => Some(at.saturating_sub(1)),
        KeyCode::Right => Some((at + 1).min(last)),
        _ => None,
    };
    match moved {
        Some(moved) => {
            *cursor = moved;
            true
        }
        None => false,
    }
}

/// Where a grid's cursor lands as the focus comes in: its first row, or its last coming
/// from below.
fn enter_grid(sizes: &[usize], cursor: &mut usize, from_below: bool) {
    let rows = crate::widgets::panel::grid_rows(sizes);
    *cursor = crate::widgets::panel::grid_enter(&rows, *cursor, from_below);
}

/// A dropdown's keys: Enter or Space opens it on its value; open, the arrows move, Enter
/// or Space picks and Esc or Tab closes it; shut, Left and Right change the value. True
/// when the key was its.
fn dropdown_key(open: &mut Option<usize>, chosen: &mut usize, count: usize, code: KeyCode) -> bool {
    let last = count.saturating_sub(1);
    match (*open, code) {
        (Some(at), KeyCode::Up) => *open = Some(at.saturating_sub(1)),
        (Some(at), KeyCode::Down) => *open = Some((at + 1).min(last)),
        (Some(at), KeyCode::Enter | KeyCode::Char(' ')) => {
            *chosen = at.min(last);
            *open = None;
        }
        (Some(_), KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab) => *open = None,
        (Some(_), _) => {}
        (None, KeyCode::Enter | KeyCode::Char(' ')) if count > 0 => *open = Some(*chosen),
        (None, KeyCode::Left) => *chosen = chosen.saturating_sub(1),
        (None, KeyCode::Right) => *chosen = (*chosen + 1).min(last),
        _ => return false,
    }
    true
}

/// The keys every form's walk answers: Up, Down, Tab and Shift+Tab between its rows;
/// Left and Right between its two buttons; Enter on them; Esc anywhere. `None`: the key
/// is the focused row's.
fn walk_key<R: Copy + PartialEq>(
    rows: &[R],
    focus: &mut R,
    cancel: R,
    submit: R,
    key: KeyEvent,
) -> Option<FormKey> {
    match key.code {
        KeyCode::Esc => return Some(FormKey::Cancel),
        KeyCode::Tab | KeyCode::Down => *focus = walk(rows, *focus, 1),
        KeyCode::BackTab | KeyCode::Up => *focus = walk(rows, *focus, -1),
        KeyCode::Left | KeyCode::Right if *focus == cancel || *focus == submit => {
            *focus = if *focus == cancel { submit } else { cancel };
        }
        // The only Enter that submits.
        KeyCode::Enter if *focus == submit => return Some(FormKey::Submit),
        KeyCode::Enter if *focus == cancel => return Some(FormKey::Cancel),
        _ => return None,
    }
    Some(FormKey::Stay)
}

fn moves_focus(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down | KeyCode::Esc
    )
}

/// A connection a profile can use, as the forms offer it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectionChoice {
    pub name: String,
    pub environment: String,
    /// Why MCP never writes to it: agents still read it.
    pub refused: Option<String>,
    pub postgres: bool,
}

/// An agent as the forms offer it.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentChoice {
    pub client: McpClient,
    /// `set up · assistant`, `found`, `not found here`.
    pub status: String,
    /// The file its config is written to.
    pub path: String,
    /// It has a place for a skill file.
    pub skill: bool,
    pub found: bool,
    /// The profile its config starts Dexo with now.
    pub uses: Option<String>,
    /// Its file cannot be read, and is left alone: why.
    pub unusable: Option<String>,
}

impl AgentChoice {
    pub fn from_clients(clients: &McpClients) -> Vec<Self> {
        clients
            .clients
            .iter()
            .map(|row| Self {
                client: row.client,
                status: row.status(),
                path: row.path.clone(),
                skill: row.skill.is_some(),
                found: row.found,
                uses: row.profile().map(str::to_string),
                unusable: match &row.state {
                    dexo_app::mcp::clients::ClientState::Unusable(why) => Some(why.clone()),
                    _ => None,
                },
            })
            .collect()
    }
}

/// The tables of one of the form's connections, as Dexo last read its catalog.
#[derive(Clone, Debug, PartialEq)]
pub enum TableState {
    Reading,
    /// Each table's selector: `database.schema.table`, or `database.table`.
    Read(Vec<String>),
    /// Dexo has never read the connection's catalog.
    NeverRead,
}

/// A group of the tables grid: its title, and each box as (selector, shown, checked).
pub type TableGroup = (String, Vec<(String, String, bool)>);

/// New profile and Edit's rows, in the order the focus walks them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileRow {
    Name,
    Connections,
    Reads,
    Sees,
    Tables,
    Agent,
    Skill,
    Cancel,
    Save,
}

/// A profile as New profile or Edit saves it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfileSave {
    pub name: String,
    /// A profile to make; else the one named, changed.
    pub new: bool,
    pub connections: Vec<String>,
    /// Read-only SQL besides browsing.
    pub reads: bool,
    /// Its selectors, written over the ones it had.
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    /// The agent to start Dexo with it, made new, and whether with a skill file.
    pub agent: Option<(McpClient, bool)>,
}

/// New profile (`n`) and Edit (Enter).
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileForm {
    pub editing: bool,
    pub name: TextInput,
    /// The saved connections MCP serves, and whether the profile uses each.
    pub connections: Vec<(ConnectionChoice, bool)>,
    pub conn_cursor: usize,
    pub sql: bool,
    pub sees: Sees,
    /// The selectors the boxes stand for: hidden (Except) or the only ones seen (Only).
    pub checked: Vec<String>,
    /// Seeing only some, the deny rules made with `dexo mcp allow --deny`, kept.
    pub kept: Vec<String>,
    /// Each connection's tables, once asked for.
    pub tables: Vec<(String, TableState)>,
    pub table_cursor: usize,
    pub agents: Vec<AgentChoice>,
    /// None (0), or an agent: its place in `agents`, plus one.
    pub agent: usize,
    pub open: Option<usize>,
    pub skill: bool,
    pub focus: ProfileRow,
    pub error: Option<(ProfileRow, String)>,
    /// Saved, waiting to hear how it went.
    pub busy: bool,
}

impl ProfileForm {
    /// A new profile: named `assistant` or the next free name, on the connection in use,
    /// reading SQL, seeing everything, for the first agent here no profile starts.
    pub fn new(
        taken: &[String],
        connections: Vec<ConnectionChoice>,
        in_use: &str,
        agents: Vec<AgentChoice>,
    ) -> Self {
        let name = std::iter::once("assistant".to_string())
            .chain((2..).map(|n| format!("assistant-{n}")))
            .find(|name| !taken.contains(name))
            .unwrap_or_default();
        let any_checked = connections.iter().any(|choice| choice.name == in_use);
        let connections = connections
            .into_iter()
            .enumerate()
            .map(|(at, choice)| {
                let on = if any_checked {
                    choice.name == in_use
                } else {
                    at == 0
                };
                (choice, on)
            })
            .collect();
        let agent = agents
            .iter()
            .position(|agent| agent.found && agent.uses.is_none() && agent.unusable.is_none())
            .map_or(0, |at| at + 1);
        Self {
            editing: false,
            name: TextInput::new(name),
            connections,
            conn_cursor: 0,
            sql: true,
            sees: Sees::All,
            checked: Vec::new(),
            kept: Vec::new(),
            tables: Vec::new(),
            table_cursor: 0,
            agents,
            agent,
            open: None,
            skill: true,
            focus: ProfileRow::Name,
            error: None,
            busy: false,
        }
    }

    /// The profile as it is, to change: its connections checked -- one it lists that MCP
    /// no longer serves stays, checked, so saving does not drop it unseen.
    pub fn edit(profile: &McpProfileSummary, connections: Vec<ConnectionChoice>) -> Self {
        let uses = |name: &str| {
            !profile.any
                && profile
                    .connections
                    .iter()
                    .any(|connection| connection.name == name)
        };
        let mut connections: Vec<(ConnectionChoice, bool)> = connections
            .into_iter()
            .map(|choice| {
                let on = uses(&choice.name);
                (choice, on)
            })
            .collect();
        if !profile.any {
            for line in &profile.connections {
                if !connections
                    .iter()
                    .any(|(choice, _)| choice.name == line.name)
                {
                    connections.push((
                        ConnectionChoice {
                            name: line.name.clone(),
                            environment: line.environment.clone(),
                            refused: line.refused.clone(),
                            postgres: line.postgres,
                        },
                        true,
                    ));
                }
            }
        }
        let (sees, checked, kept) = sees_of(&profile.scopes);
        Self {
            editing: true,
            name: TextInput::new(profile.name.clone()),
            connections,
            conn_cursor: 0,
            sql: profile.raw_read,
            sees,
            checked,
            kept,
            tables: Vec::new(),
            table_cursor: 0,
            agents: Vec::new(),
            agent: 0,
            open: None,
            skill: false,
            focus: ProfileRow::Connections,
            error: None,
            busy: false,
        }
    }

    pub fn rows(&self) -> Vec<ProfileRow> {
        let mut rows = Vec::new();
        if !self.editing {
            rows.push(ProfileRow::Name);
        }
        rows.extend([ProfileRow::Connections, ProfileRow::Reads, ProfileRow::Sees]);
        if self.sees != Sees::All {
            rows.push(ProfileRow::Tables);
        }
        if !self.editing {
            rows.push(ProfileRow::Agent);
            if self.agent_choice().is_some_and(|agent| agent.skill) {
                rows.push(ProfileRow::Skill);
            }
        }
        rows.extend([ProfileRow::Cancel, ProfileRow::Save]);
        rows
    }

    pub fn index_of(&self, row: ProfileRow) -> usize {
        self.rows().iter().position(|at| *at == row).unwrap_or(0)
    }

    pub fn agent_choice(&self) -> Option<&AgentChoice> {
        self.agent.checked_sub(1).and_then(|at| self.agents.get(at))
    }

    pub fn picked_connections(&self) -> Vec<&ConnectionChoice> {
        self.connections
            .iter()
            .filter(|(_, on)| *on)
            .map(|(choice, _)| choice)
            .collect()
    }

    /// The connections whose tables are to be read: checked, while the profile sees only
    /// some, and not asked for yet. Each is then marked as being read.
    pub fn wants_tables(&mut self) -> Vec<(String, bool)> {
        if self.sees == Sees::All {
            return Vec::new();
        }
        let wanted: Vec<(String, bool)> = self
            .picked_connections()
            .into_iter()
            .filter(|choice| !self.tables.iter().any(|(name, _)| *name == choice.name))
            .map(|choice| (choice.name.clone(), choice.postgres))
            .collect();
        for (name, _) in &wanted {
            self.tables.push((name.clone(), TableState::Reading));
        }
        wanted
    }

    pub fn take_tables(&mut self, connection: &str, tables: Option<Vec<String>>) {
        if let Some((_, state)) = self.tables.iter_mut().find(|(name, _)| name == connection) {
            *state = match tables {
                Some(tables) => TableState::Read(tables),
                None => TableState::NeverRead,
            };
        }
    }

    /// The boxes of the grid: each checked connection's tables read, named without their
    /// database, then the selectors checked that are none of them -- a schema's `*` made
    /// with `dexo mcp allow` -- as written. Each as (selector, shown, checked).
    pub fn table_groups(&self) -> Vec<TableGroup> {
        let mut seen = Vec::new();
        let mut groups = Vec::new();
        for choice in self.picked_connections() {
            let Some((_, TableState::Read(tables))) =
                self.tables.iter().find(|(name, _)| *name == choice.name)
            else {
                continue;
            };
            let items = tables
                .iter()
                .map(|table| {
                    seen.push(table.clone());
                    let shown = table
                        .split_once('.')
                        .map_or(table.as_str(), |(_, rest)| rest);
                    (
                        table.clone(),
                        shown.to_string(),
                        self.checked.contains(table),
                    )
                })
                .collect();
            groups.push((choice.name.clone(), items));
        }
        let others: Vec<(String, String, bool)> = self
            .checked
            .iter()
            .filter(|selector| !seen.contains(selector))
            .map(|selector| (selector.clone(), selector.clone(), true))
            .collect();
        if !others.is_empty() {
            groups.push(("other rules".into(), others));
        }
        groups
    }

    fn table_sizes(&self) -> Vec<usize> {
        self.table_groups()
            .iter()
            .map(|(_, items)| items.len())
            .collect()
    }

    fn table_at(&self, at: usize) -> Option<String> {
        self.table_groups()
            .into_iter()
            .flat_map(|(_, items)| items)
            .nth(at)
            .map(|(selector, ..)| selector)
    }

    /// Flips a box: a connection, a table, by its place.
    pub fn toggle(&mut self, row: ProfileRow, at: usize) {
        self.error = None;
        match row {
            ProfileRow::Connections => {
                if let Some((_, on)) = self.connections.get_mut(at) {
                    *on = !*on;
                }
                self.conn_cursor = at;
            }
            ProfileRow::Tables => {
                if let Some(table) = self.table_at(at) {
                    match self.checked.iter().position(|t| *t == table) {
                        Some(found) => {
                            self.checked.remove(found);
                        }
                        None => self.checked.push(table),
                    }
                }
                self.table_cursor = at;
            }
            _ => {}
        }
    }

    /// Picks option `option` of `row`: a segment, an agent, a box, the toggle.
    pub fn pick(&mut self, row: ProfileRow, option: usize) {
        self.focus = row;
        self.error = None;
        match row {
            ProfileRow::Reads => self.sql = option == 1,
            ProfileRow::Sees => {
                self.sees = [Sees::All, Sees::Except, Sees::Only][option.min(2)];
            }
            ProfileRow::Agent => {
                self.agent = option.min(self.agents.len());
                self.open = None;
            }
            ProfileRow::Skill => self.skill = !self.skill,
            ProfileRow::Connections | ProfileRow::Tables => self.toggle(row, option),
            _ => {}
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> FormKey {
        if self.focus == ProfileRow::Agent
            && (self.open.is_some() || !moves_focus(key.code))
            && dropdown_key(
                &mut self.open,
                &mut self.agent,
                self.agents.len() + 1,
                key.code,
            )
        {
            return FormKey::Stay;
        }
        match (self.focus, key.code) {
            (ProfileRow::Connections, code)
                if grid_key(&[self.connections.len()], &mut self.conn_cursor, code) =>
            {
                return FormKey::Stay;
            }
            (ProfileRow::Tables, code)
                if grid_key(&self.table_sizes(), &mut self.table_cursor, code) =>
            {
                return FormKey::Stay;
            }
            (ProfileRow::Connections, KeyCode::Char(' ') | KeyCode::Enter) => {
                self.toggle(ProfileRow::Connections, self.conn_cursor);
                return FormKey::Stay;
            }
            (ProfileRow::Tables, KeyCode::Char(' ') | KeyCode::Enter) => {
                self.toggle(ProfileRow::Tables, self.table_cursor);
                return FormKey::Stay;
            }
            (ProfileRow::Reads, KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')) => {
                self.sql = !self.sql;
                return FormKey::Stay;
            }
            (ProfileRow::Sees, KeyCode::Left) => {
                self.sees = match self.sees {
                    Sees::Only => Sees::Except,
                    _ => Sees::All,
                };
                return FormKey::Stay;
            }
            (ProfileRow::Sees, KeyCode::Right) => {
                self.sees = match self.sees {
                    Sees::All => Sees::Except,
                    _ => Sees::Only,
                };
                return FormKey::Stay;
            }
            (ProfileRow::Sees, KeyCode::Char(' ')) => {
                self.sees = match self.sees {
                    Sees::All => Sees::Except,
                    Sees::Except => Sees::Only,
                    Sees::Only => Sees::All,
                };
                return FormKey::Stay;
            }
            (ProfileRow::Skill, KeyCode::Char(' ') | KeyCode::Enter) => {
                self.skill = !self.skill;
                return FormKey::Stay;
            }
            (ProfileRow::Name, code) if !moves_focus(code) && code != KeyCode::Enter => {
                self.name.handle_key(key);
                self.error = None;
                return FormKey::Stay;
            }
            _ => {}
        }
        let rows = self.rows();
        let was = self.focus;
        let outcome = match walk_key(
            &rows,
            &mut self.focus,
            ProfileRow::Cancel,
            ProfileRow::Save,
            key,
        ) {
            Some(outcome) => outcome,
            None if key.code == KeyCode::Enter => {
                self.focus = walk(&rows, self.focus, 1);
                FormKey::Stay
            }
            None => FormKey::Stay,
        };
        // Into a grid from above or below: its first row, or its last.
        if self.focus != was {
            let from_below = matches!(key.code, KeyCode::Up | KeyCode::BackTab);
            match self.focus {
                ProfileRow::Connections => {
                    enter_grid(&[self.connections.len()], &mut self.conn_cursor, from_below)
                }
                ProfileRow::Tables => {
                    let sizes = self.table_sizes();
                    enter_grid(&sizes, &mut self.table_cursor, from_below);
                }
                _ => {}
            }
        }
        outcome
    }

    /// The input the focus is on, for a paste.
    pub fn focused_input(&mut self) -> Option<&mut TextInput> {
        (self.focus == ProfileRow::Name && !self.editing).then_some(&mut self.name)
    }

    pub fn hints(&self) -> &'static [(&'static str, &'static str)] {
        if self.open.is_some() {
            return &[("↑/↓", "choose"), ("Enter", "pick"), ("Esc", "close")];
        }
        match self.focus {
            ProfileRow::Name => &[("Tab", "next"), ("Esc", "cancel")],
            ProfileRow::Connections | ProfileRow::Tables => &[
                ("↑/↓/←/→", "move"),
                ("Space", "check"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            ProfileRow::Reads | ProfileRow::Sees => {
                &[("←/→", "choose"), ("Tab", "next"), ("Esc", "cancel")]
            }
            ProfileRow::Agent => &[
                ("Enter", "open"),
                ("←/→", "change"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            ProfileRow::Skill => &[("Space", "toggle"), ("Tab", "next"), ("Esc", "cancel")],
            ProfileRow::Save => &[("Enter", "save"), ("←/→", "switch"), ("Esc", "cancel")],
            ProfileRow::Cancel => &[("Enter", "cancel"), ("←/→", "switch")],
        }
    }

    /// What Save sends, or the row that stops it and why.
    pub fn request(&self, taken: &[String]) -> Result<ProfileSave, (ProfileRow, String)> {
        let name = self.name.trim().to_string();
        if !self.editing {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
            {
                return Err((
                    ProfileRow::Name,
                    "a name takes letters, digits, - and _: it goes into the agent's config".into(),
                ));
            }
            if taken.contains(&name) {
                return Err((ProfileRow::Name, format!("{name} is a profile already")));
            }
        }
        let connections: Vec<String> = self
            .picked_connections()
            .into_iter()
            .map(|choice| choice.name.clone())
            .collect();
        if connections.is_empty() {
            return Err((
                ProfileRow::Connections,
                if self.connections.is_empty() {
                    "no saved Postgres or MySQL connection to give it: MCP serves those".into()
                } else {
                    "check at least one connection".into()
                },
            ));
        }
        let (allow, deny) = match self.sees {
            Sees::All => (vec!["*".to_string()], Vec::new()),
            Sees::Except if self.checked.is_empty() => {
                return Err((ProfileRow::Tables, "check the tables it may not see".into()));
            }
            Sees::Except => (vec!["*".to_string()], self.checked.clone()),
            Sees::Only if self.checked.is_empty() => {
                return Err((ProfileRow::Tables, "check the tables it may see".into()));
            }
            Sees::Only => (self.checked.clone(), self.kept.clone()),
        };
        let agent = match self.agent_choice() {
            Some(agent) => {
                if let Some(why) = &agent.unusable {
                    return Err((
                        ProfileRow::Agent,
                        format!("{} is left as it is: {why}", agent.path),
                    ));
                }
                Some((agent.client, self.skill && agent.skill))
            }
            None => None,
        };
        Ok(ProfileSave {
            name,
            new: !self.editing,
            connections,
            reads: self.sql,
            allow,
            deny,
            agent,
        })
    }

    /// Puts the focus on the row a refusal is about.
    pub fn fail(&mut self, message: String) {
        let lower = message.to_lowercase();
        let row = [
            ("name", ProfileRow::Name),
            ("connection", ProfileRow::Connections),
            ("table", ProfileRow::Tables),
            ("selector", ProfileRow::Tables),
            ("config", ProfileRow::Agent),
            ("file", ProfileRow::Agent),
        ]
        .into_iter()
        .find(|(word, _)| lower.contains(word))
        .map(|(_, row)| row)
        .filter(|row| self.rows().contains(row))
        .unwrap_or(self.focus);
        self.focus = row;
        self.open = None;
        self.error = Some((row, message));
    }
}

/// How long a write access lasts: what `--expires` takes, the choice, the sentence's end.
pub const LASTS: [(&str, &str, &str); 6] = [
    ("15m", "15m", "for 15 minutes"),
    ("1h", "1h", "for 1 hour"),
    ("2h", "2h", "for 2 hours"),
    ("8h", "8h", "for 8 hours"),
    ("24h", "24h", "for 24 hours"),
    ("never", "Until revoked", "until you revoke it"),
];

/// Writes' rows, in the order the focus walks them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WritesRow {
    Connection,
    Kinds,
    Lasts,
    Cancel,
    Create,
}

/// Writes (`w`): what the profile may change on one connection, and for how long; every
/// write asks.
#[derive(Clone, Debug, PartialEq)]
pub struct WritesForm {
    pub profile: String,
    /// The profile's connections that take writes.
    pub offered: Vec<ConnectionLine>,
    /// The rest, each with why it is not offered.
    pub left_out: Vec<(String, String)>,
    pub connection: usize,
    pub open: Option<usize>,
    /// The kinds a write access can give the profile.
    pub kinds: Vec<WriteKind>,
    pub checked: Vec<WriteKind>,
    pub cursor: usize,
    pub lasts: usize,
    pub focus: WritesRow,
    pub error: Option<(WritesRow, String)>,
    pub busy: bool,
}

impl WritesForm {
    /// The form for `profile`, on the connection `on` when it takes writes; or why there
    /// is nothing to offer.
    pub fn new(profile: &McpProfileSummary, on: Option<&str>) -> Result<Self, String> {
        let (offered, refused): (Vec<&ConnectionLine>, Vec<&ConnectionLine>) = profile
            .connections
            .iter()
            .partition(|connection| connection.refused.is_none());
        if offered.is_empty() {
            return Err(format!(
                "{} has no connection that takes writes: MCP never writes to production or read-only connections.",
                profile.name
            ));
        }
        if profile.kinds.is_empty() {
            return Err(format!(
                "{} denies every write tool: dexo mcp profile set --name {} --allow-tool data_insert",
                profile.name, profile.name
            ));
        }
        let connection = on
            .and_then(|on| offered.iter().position(|line| line.name == on))
            .unwrap_or(0);
        let mut form = Self {
            profile: profile.name.clone(),
            left_out: refused
                .iter()
                .map(|line| (line.name.clone(), line.refused.clone().unwrap_or_default()))
                .collect(),
            offered: offered.into_iter().cloned().collect(),
            connection,
            open: None,
            kinds: profile.kinds.clone(),
            checked: vec![WriteKind::Rows],
            cursor: 0,
            lasts: 1,
            focus: WritesRow::Kinds,
            error: None,
            busy: false,
        };
        form.load_current();
        Ok(form)
    }

    /// What the picked connection may write now, to change it.
    fn load_current(&mut self) {
        let kinds = self.kinds.clone();
        if let Some(access) = self.current() {
            let until_revoked = access.until_revoked;
            self.checked = access
                .kinds
                .iter()
                .copied()
                .filter(|kind| kinds.contains(kind))
                .collect();
            if until_revoked {
                self.lasts = LASTS.len() - 1;
            }
        } else {
            self.checked.retain(|kind| kinds.contains(kind));
        }
    }

    /// The write access the picked connection has now: what Create replaces.
    pub fn current(&self) -> Option<&Access> {
        self.offered.get(self.connection)?.access.as_ref()
    }

    pub fn connection_name(&self) -> String {
        self.offered
            .get(self.connection)
            .map(|line| line.name.clone())
            .unwrap_or_default()
    }

    pub fn rows(&self) -> Vec<WritesRow> {
        vec![
            WritesRow::Connection,
            WritesRow::Kinds,
            WritesRow::Lasts,
            WritesRow::Cancel,
            WritesRow::Create,
        ]
    }

    pub fn index_of(&self, row: WritesRow) -> usize {
        self.rows().iter().position(|at| *at == row).unwrap_or(0)
    }

    pub fn until_revoked(&self) -> bool {
        self.lasts == LASTS.len() - 1
    }

    pub fn toggle(&mut self, at: usize) {
        self.error = None;
        self.cursor = at.min(self.kinds.len().saturating_sub(1));
        if let Some(kind) = self.kinds.get(self.cursor).copied() {
            match self.checked.iter().position(|on| *on == kind) {
                Some(found) => {
                    self.checked.remove(found);
                }
                None => self.checked.push(kind),
            }
        }
    }

    /// Picks option `option` of `row`.
    pub fn pick(&mut self, row: WritesRow, option: usize) {
        self.focus = row;
        self.error = None;
        match row {
            WritesRow::Connection => {
                self.connection = option.min(self.offered.len().saturating_sub(1));
                self.open = None;
                self.load_current();
            }
            WritesRow::Kinds => self.toggle(option),
            WritesRow::Lasts => self.lasts = option.min(LASTS.len() - 1),
            _ => {}
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> FormKey {
        if self.focus == WritesRow::Connection && (self.open.is_some() || !moves_focus(key.code)) {
            let before = self.connection;
            if dropdown_key(
                &mut self.open,
                &mut self.connection,
                self.offered.len(),
                key.code,
            ) {
                if self.connection != before {
                    self.load_current();
                }
                return FormKey::Stay;
            }
        }
        match (self.focus, key.code) {
            (WritesRow::Kinds, code) if grid_key(&[self.kinds.len()], &mut self.cursor, code) => {
                return FormKey::Stay;
            }
            (WritesRow::Kinds, KeyCode::Char(' ') | KeyCode::Enter) => {
                self.toggle(self.cursor);
                return FormKey::Stay;
            }
            (WritesRow::Lasts, KeyCode::Left) => {
                self.lasts = self.lasts.saturating_sub(1);
                return FormKey::Stay;
            }
            (WritesRow::Lasts, KeyCode::Right) => {
                self.lasts = (self.lasts + 1).min(LASTS.len() - 1);
                return FormKey::Stay;
            }
            (WritesRow::Lasts, KeyCode::Char(' ')) => {
                self.lasts = (self.lasts + 1) % LASTS.len();
                return FormKey::Stay;
            }
            _ => {}
        }
        let rows = self.rows();
        let was = self.focus;
        let outcome = match walk_key(
            &rows,
            &mut self.focus,
            WritesRow::Cancel,
            WritesRow::Create,
            key,
        ) {
            Some(outcome) => outcome,
            None if key.code == KeyCode::Enter => {
                self.focus = walk(&rows, self.focus, 1);
                FormKey::Stay
            }
            None => FormKey::Stay,
        };
        if self.focus == WritesRow::Kinds && was != WritesRow::Kinds {
            let from_below = matches!(key.code, KeyCode::Up | KeyCode::BackTab);
            enter_grid(&[self.kinds.len()], &mut self.cursor, from_below);
        }
        outcome
    }

    pub fn hints(&self) -> &'static [(&'static str, &'static str)] {
        if self.open.is_some() {
            return &[("↑/↓", "choose"), ("Enter", "pick"), ("Esc", "close")];
        }
        match self.focus {
            WritesRow::Connection => &[
                ("Enter", "open"),
                ("←/→", "change"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            WritesRow::Kinds => &[
                ("↑/↓/←/→", "move"),
                ("Space", "check"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            WritesRow::Lasts => &[("←/→", "choose"), ("Tab", "next"), ("Esc", "cancel")],
            WritesRow::Create => &[("Enter", "allow"), ("←/→", "switch"), ("Esc", "cancel")],
            WritesRow::Cancel => &[("Enter", "cancel"), ("←/→", "switch")],
        }
    }

    /// What it would let the profile do, in a sentence.
    pub fn result(&self) -> Option<String> {
        let changes: Vec<&str> = [(WriteKind::Rows, "rows"), (WriteKind::Schema, "the schema")]
            .into_iter()
            .filter(|(kind, _)| self.checked.contains(kind))
            .map(|(_, word)| word)
            .collect();
        let mut does = Vec::new();
        if !changes.is_empty() {
            does.push(format!("change {}", changes.join(" and ")));
        }
        if self.checked.contains(&WriteKind::Sessions) {
            does.push("end sessions".to_string());
        }
        (!does.is_empty()).then(|| {
            format!(
                "{} may {} in {}, asking you before each write, {}.",
                self.profile,
                does.join(" and "),
                self.connection_name(),
                LASTS[self.lasts].2
            )
        })
    }

    /// The request `dexo mcp grant create` would make with the same answers, or the row
    /// that stops it and why.
    pub fn request(&self) -> Result<dexo_app::mcp::GrantRequest, (WritesRow, String)> {
        let kinds: Vec<WriteKind> = WriteKind::ALL
            .into_iter()
            .filter(|kind| self.checked.contains(kind))
            .collect();
        if kinds.is_empty() {
            return Err((WritesRow::Kinds, "check what it may change".into()));
        }
        Ok(dexo_app::mcp::GrantRequest {
            connection: self.connection_name(),
            kinds,
            expires: LASTS[self.lasts].0.to_string(),
            ask_secs: None,
        })
    }

    pub fn fail(&mut self, message: String) {
        let lower = message.to_lowercase();
        let row = if lower.contains("connection") {
            WritesRow::Connection
        } else if lower.contains("time") || lower.contains("lasts") {
            WritesRow::Lasts
        } else {
            WritesRow::Kinds
        };
        self.focus = row;
        self.open = None;
        self.error = Some((row, message));
    }
}

/// Connect agent's rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentRow {
    Agent,
    Skill,
    Cancel,
    Connect,
}

/// Connect agent (`a`): an agent's config written to start Dexo with this profile.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentForm {
    pub profile: String,
    /// Connecting enables a disabled profile, and the form says so.
    pub enabled: bool,
    pub agents: Vec<AgentChoice>,
    pub agent: usize,
    pub open: Option<usize>,
    pub skill: bool,
    pub focus: AgentRow,
    pub error: Option<String>,
    pub busy: bool,
}

impl AgentForm {
    /// The form on `agent`, or on one here that this profile does not start yet.
    pub fn new(
        profile: &McpProfileSummary,
        agents: Vec<AgentChoice>,
        agent: Option<McpClient>,
    ) -> Self {
        let free = |choice: &AgentChoice| choice.uses.as_deref() != Some(profile.name.as_str());
        let at = agent
            .and_then(|agent| agents.iter().position(|choice| choice.client == agent))
            .or_else(|| {
                agents
                    .iter()
                    .position(|choice| free(choice) && choice.found)
            })
            .or_else(|| agents.iter().position(free))
            .unwrap_or(0);
        Self {
            profile: profile.name.clone(),
            enabled: profile.enabled,
            agents,
            agent: at,
            open: None,
            skill: true,
            focus: AgentRow::Agent,
            error: None,
            busy: false,
        }
    }

    pub fn choice(&self) -> Option<&AgentChoice> {
        self.agents.get(self.agent)
    }

    pub fn rows(&self) -> Vec<AgentRow> {
        let mut rows = vec![AgentRow::Agent];
        if self.choice().is_some_and(|choice| choice.skill) {
            rows.push(AgentRow::Skill);
        }
        rows.extend([AgentRow::Cancel, AgentRow::Connect]);
        rows
    }

    pub fn index_of(&self, row: AgentRow) -> usize {
        self.rows().iter().position(|at| *at == row).unwrap_or(0)
    }

    pub fn pick(&mut self, row: AgentRow, option: usize) {
        self.focus = row;
        self.error = None;
        match row {
            AgentRow::Agent => {
                self.agent = option.min(self.agents.len().saturating_sub(1));
                self.open = None;
            }
            AgentRow::Skill => self.skill = !self.skill,
            _ => {}
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> FormKey {
        if self.focus == AgentRow::Agent
            && (self.open.is_some() || !moves_focus(key.code))
            && dropdown_key(&mut self.open, &mut self.agent, self.agents.len(), key.code)
        {
            self.error = None;
            return FormKey::Stay;
        }
        if self.focus == AgentRow::Skill && matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter)
        {
            self.skill = !self.skill;
            return FormKey::Stay;
        }
        let rows = self.rows();
        match walk_key(
            &rows,
            &mut self.focus,
            AgentRow::Cancel,
            AgentRow::Connect,
            key,
        ) {
            Some(outcome) => outcome,
            None if key.code == KeyCode::Enter => {
                self.focus = walk(&rows, self.focus, 1);
                FormKey::Stay
            }
            None => FormKey::Stay,
        }
    }

    pub fn hints(&self) -> &'static [(&'static str, &'static str)] {
        if self.open.is_some() {
            return &[("↑/↓", "choose"), ("Enter", "pick"), ("Esc", "close")];
        }
        match self.focus {
            AgentRow::Agent => &[
                ("Enter", "open"),
                ("←/→", "change"),
                ("y", "copy its command"),
                ("Esc", "cancel"),
            ],
            AgentRow::Skill => &[("Space", "toggle"), ("Tab", "next"), ("Esc", "cancel")],
            AgentRow::Connect => &[("Enter", "connect"), ("←/→", "switch"), ("Esc", "cancel")],
            AgentRow::Cancel => &[("Enter", "cancel"), ("←/→", "switch")],
        }
    }

    /// The agent to write and whether with a skill file, or why not.
    pub fn request(&self) -> Result<(McpClient, bool), String> {
        let choice = self.choice().ok_or("no agent to connect")?;
        if let Some(why) = &choice.unusable {
            return Err(format!(
                "{} is left as it is: {why}. Fix it or move it away, then connect again.",
                choice.path
            ));
        }
        Ok((choice.client, self.skill && choice.skill))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    fn line(name: &str, refused: Option<&str>) -> ConnectionLine {
        ConnectionLine {
            name: name.into(),
            environment: "local".into(),
            refused: refused.map(str::to_string),
            postgres: true,
            access: None,
        }
    }

    fn profile() -> McpProfileSummary {
        McpProfileSummary {
            name: "assistant".into(),
            enabled: true,
            raw_read: true,
            scopes: vec!["allow *".into(), "deny app.public.secrets".into()],
            any: false,
            connections: vec![
                line("pg-dev", None),
                line("pg-prod", Some("MCP never writes to production")),
            ],
            kinds: WriteKind::ALL.to_vec(),
        }
    }

    /// What a profile sees is read from its selectors, and saved as the same ones; a deny
    /// rule made by hand is kept while it sees only some.
    #[test]
    fn what_a_profile_sees_round_trips() {
        assert_eq!(sees_of(&["allow *".into()]).0, Sees::All);
        let (sees, checked, _) = sees_of(&profile().scopes);
        assert_eq!(sees, Sees::Except);
        assert_eq!(checked, ["app.public.secrets"]);
        let scopes = vec![
            "allow app.public.*".to_string(),
            "deny app.public.secrets".into(),
        ];
        let (sees, checked, kept) = sees_of(&scopes);
        assert_eq!(sees, Sees::Only);
        assert_eq!(
            sees_words(sees, &checked, &kept),
            "only app.public.*, never app.public.secrets"
        );
        let summary = McpProfileSummary {
            scopes,
            ..profile()
        };
        let form = ProfileForm::edit(&summary, Vec::new());
        let save = form.request(&[]).unwrap();
        assert_eq!(save.allow, ["app.public.*"]);
        assert_eq!(save.deny, ["app.public.secrets"]);
    }

    /// New profile: Enter creates only on its button, a bad name is refused on its row,
    /// and a profile that sees only some must check some.
    #[test]
    fn new_profile_creates_only_on_its_button() {
        let choice = ConnectionChoice {
            name: "pg-dev".into(),
            ..ConnectionChoice::default()
        };
        let mut form = ProfileForm::new(&["assistant".into()], vec![choice], "pg-dev", Vec::new());
        assert_eq!(form.name.as_str(), "assistant-2");
        assert_eq!(form.key(key(KeyCode::Enter)), FormKey::Stay);
        assert_eq!(form.focus, ProfileRow::Connections);
        form.focus = ProfileRow::Sees;
        form.key(key(KeyCode::Right));
        assert_eq!(form.sees, Sees::Except);
        assert!(form.rows().contains(&ProfileRow::Tables));
        assert_eq!(form.request(&[]).unwrap_err().0, ProfileRow::Tables);
        form.sees = Sees::All;
        form.focus = ProfileRow::Save;
        assert_eq!(form.key(key(KeyCode::Enter)), FormKey::Submit);
        let save = form.request(&["assistant".into()]).unwrap();
        assert_eq!(save.allow, ["*"]);
        assert!(save.new);
        form.name.set_text("no spaces");
        assert_eq!(form.request(&[]).unwrap_err().0, ProfileRow::Name);
    }

    /// Writes offers the connections that take writes, opens on what the walked one has,
    /// and asks for something to change.
    #[test]
    fn writes_offers_only_what_takes_writes() {
        let mut summary = profile();
        summary.connections[0].access = Some(Access {
            kinds: vec![WriteKind::Schema],
            until_revoked: true,
            ..Access::default()
        });
        let mut form = WritesForm::new(&summary, Some("pg-dev")).unwrap();
        assert_eq!(form.offered.len(), 1);
        assert_eq!(form.left_out[0].0, "pg-prod");
        assert_eq!(form.checked, [WriteKind::Schema]);
        assert!(form.until_revoked());
        form.toggle(1);
        assert_eq!(form.request().unwrap_err().0, WritesRow::Kinds);
        form.toggle(0);
        let request = form.request().unwrap();
        assert_eq!(request.kinds, [WriteKind::Rows]);
        assert_eq!(request.expires, "never");
        summary.connections.remove(0);
        assert!(WritesForm::new(&summary, None).is_err());
    }

    /// Enabling asks first, disabling does not; stopping every write says how many go,
    /// and has nothing to ask when nothing may write.
    #[test]
    fn enabling_and_stopping_all_ask_first() {
        let mut screen = McpProfilesScreen {
            profiles: vec![McpProfileSummary {
                enabled: false,
                ..profile()
            }],
            ..McpProfilesScreen::default()
        };
        assert_eq!(screen.toggle_selected(), None);
        assert_eq!(
            screen.confirm.as_ref().map(|confirm| confirm.focus),
            Some(FooterFocus::Cancel)
        );
        screen.confirm = None;
        screen.ask_revoke_all();
        assert!(screen.confirm.is_none() && screen.status.contains("nothing to stop"));
        screen.profiles[0].connections[0].access = Some(Access::default());
        screen.ask_revoke_all();
        assert!(matches!(
            screen.confirm.as_ref().map(|confirm| &confirm.kind),
            Some(McpConfirmKind::RevokeAll {
                grants: 1,
                profiles: 1
            })
        ));
    }
}
