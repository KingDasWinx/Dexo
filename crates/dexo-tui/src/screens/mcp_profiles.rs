//! MCP Profiles: the profiles agents connect through, what each allows, and the grants
//! that let one write for a while. A profile's rules are made with `dexo mcp`; this
//! screen enables, disables and deletes profiles, makes grants and takes them back.

use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpProfileSummary {
    pub name: String,
    pub enabled: bool,
    /// The connections the profile may use; none listed means any.
    pub connections: Vec<String>,
    /// The profile offers `query_execute_read`, a raw read-only SQL statement.
    pub raw_read: bool,
    pub scopes: Vec<String>,
    pub tools: Vec<String>,
    /// Live grants for this profile. The screen shows the selected profile's, which
    /// is what makes `r` (revoke the selected profile's grants) mean something on screen.
    pub grants: Vec<GrantLine>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrantLine {
    pub id: String,
    pub capability: String,
    pub tools: String,
    pub expires_in_secs: i64,
    pub connection: String,
    pub selectors: String,
    /// Above zero, each write the grant covers waits this long for a person.
    pub ask_secs: u32,
    /// It lasts until it is revoked: `expires_in_secs` means nothing.
    pub until_revoked: bool,
}

/// A span of seconds in words: `45 s`, `29 min`, `3 h`.
pub fn duration_words(secs: i64) -> String {
    match secs {
        i64::MIN..=0 => "now".into(),
        1..=89 => format!("{secs} s"),
        90..=5399 => format!("{} min", (secs + 30) / 60),
        _ => format!("{} h", (secs + 1800) / 3600),
    }
}

impl GrantLine {
    /// What the grant allows, where and for how long, in a sentence.
    pub fn words(&self) -> String {
        let how = if self.ask_secs > 0 {
            format!(
                "asks before each write ({})",
                duration_words(i64::from(self.ask_secs))
            )
        } else {
            "one write, then it is spent".into()
        };
        let objects = if self.selectors.is_empty() {
            String::new()
        } else {
            format!(" on {}", self.selectors)
        };
        let ends = if self.until_revoked {
            "lasts until it is revoked".to_string()
        } else {
            format!("ends in {}", duration_words(self.expires_in_secs))
        };
        format!(
            "{}{objects} ({}): {how}, {ends}",
            self.tools.replace(',', ", "),
            self.connection,
        )
    }
}

/// What a grant can allow, what that means, and the tools each allows.
const CAPABILITIES: [(&str, &str, &[&str]); 3] = [
    (
        "data_write",
        "change rows",
        &[
            "data_insert",
            "data_update",
            "data_delete",
            "data_execute_sql",
        ],
    ),
    ("ddl", "change tables and schemas", &["schema_apply_ddl"]),
    (
        "admin",
        "cancel queries, end sessions",
        &["admin_cancel_query", "admin_terminate_session"],
    ),
];

/// How long a grant lasts: what `--expires` takes, what the choice says, and how it
/// reads after "ends in".
const EXPIRES: [(&str, &str, &str); 6] = [
    ("15m", "15m", "15 minutes"),
    ("1h", "1h", "1 hour"),
    ("2h", "2h", "2 hours"),
    ("8h", "8h", "8 hours"),
    ("24h", "24h", "24 hours"),
    ("never", "Until revoked", ""),
];

/// The profiles a grant can be for, with the connections each may use and the objects
/// it sees.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileChoice {
    pub name: String,
    pub connections: Vec<String>,
    /// The selectors it allows: a grant narrows one of them.
    pub sees: Vec<String>,
}

/// A saved connection a grant can be for, with its environment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectionChoice {
    pub name: String,
    pub environment: String,
    /// Why MCP never writes to it -- production, or kept read-only -- which no grant
    /// changes: it is left out of the choices.
    pub refused: Option<String>,
    /// A PostgreSQL connection: its tables are `database.schema.table`.
    pub postgres: bool,
}

/// The tables of the grant's connection, as Dexo last read them.
#[derive(Clone, Debug, PartialEq)]
pub struct TableList {
    pub connection: String,
    pub state: TableState,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TableState {
    Reading,
    /// Each table's selector, and whether it is checked.
    Read(Vec<(String, bool)>),
    /// Dexo has never read the connection's catalog.
    NeverRead,
}

/// New grant's rows, in the order the focus walks them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantRow {
    Profile,
    Connection,
    Tools,
    /// Everything the profile sees, or only some tables.
    Tables,
    /// The tables to check, when only some.
    TablePick,
    Expires,
    Ask,
    AskSecs,
    Confirm,
    Cancel,
    Create,
}

/// What a key did to New grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormKey {
    Stay,
    Submit,
    Cancel,
}

/// New grant: what `dexo mcp grant create` asks, "ask before each write" included, in
/// the form every form follows (`docs/design.md`).
#[derive(Clone, Debug, PartialEq)]
pub struct GrantForm {
    pub profiles: Vec<ProfileChoice>,
    /// The saved connections agents can use: the choices when a profile lists none.
    pub saved: Vec<ConnectionChoice>,
    pub profile: usize,
    pub connection: usize,
    pub tools: Vec<bool>,
    /// The grant covers everything the profile sees; or only the tables checked.
    pub everything: bool,
    pub tables: Option<TableList>,
    /// The table the grid's cursor is on.
    pub table: usize,
    pub expires: usize,
    pub ask: bool,
    pub ask_secs: TextInput,
    pub confirm: TextInput,
    pub focus: GrantRow,
    /// The tool the grid's cursor is on.
    pub tool: usize,
    /// The highlighted row of the dropdown that is open.
    pub open: Option<usize>,
    pub error: Option<(GrantRow, String)>,
}

impl GrantForm {
    pub fn new(profiles: Vec<ProfileChoice>, saved: Vec<ConnectionChoice>, profile: usize) -> Self {
        let mut form = Self {
            profiles,
            saved,
            profile: 0,
            connection: 0,
            tools: Vec::new(),
            everything: true,
            tables: None,
            table: 0,
            expires: 0,
            ask: false,
            ask_secs: TextInput::new("120"),
            confirm: TextInput::default(),
            focus: GrantRow::Profile,
            tool: 0,
            open: None,
            error: None,
        };
        form.set_profile(profile);
        form.tools = vec![false; CAPABILITIES.iter().map(|(.., tools)| tools.len()).sum()];
        form
    }

    fn set_profile(&mut self, index: usize) {
        self.profile = index.min(self.profiles.len().saturating_sub(1));
        self.connection = 0;
    }

    /// What the profile sees, as written: what "everything" covers.
    pub fn sees(&self) -> Vec<String> {
        self.profiles
            .get(self.profile)
            .map(|profile| profile.sees.clone())
            .unwrap_or_default()
    }

    /// The tables of the connection chosen are to be read: only some are wanted, and
    /// they were not read for it. Its name and whether it is PostgreSQL.
    pub fn wants_tables(&self) -> Option<(String, bool)> {
        if self.everything {
            return None;
        }
        let choice = self.connection_choice()?;
        let read = self
            .tables
            .as_ref()
            .is_some_and(|tables| tables.connection == choice.name);
        (!read).then_some((choice.name, choice.postgres))
    }

    /// The connection's tables as read: those inside what the profile sees, none
    /// checked. `None`: Dexo never read them.
    pub fn take_tables(&mut self, connection: &str, tables: Option<Vec<String>>) {
        use dexo_app::mcp::{Decision, Effect, ObjectPolicy, ObjectRef, SelectorRule};
        let sees: Vec<SelectorRule> = self
            .sees()
            .iter()
            .filter_map(|selector| SelectorRule::parse(Effect::Allow, selector).ok())
            .collect();
        let Some(list) = self
            .tables
            .as_mut()
            .filter(|list| list.connection == connection)
        else {
            return;
        };
        let policy = ObjectPolicy::new(sees);
        list.state = match tables {
            Some(tables) => TableState::Read(
                tables
                    .into_iter()
                    .filter(|table| {
                        let path = table.split('.').map(str::to_string).collect();
                        policy.decide(&ObjectRef { path }) == Decision::Allow
                    })
                    .map(|table| (table, false))
                    .collect(),
            ),
            None => TableState::NeverRead,
        };
        self.table = 0;
    }

    /// The tables to check, each named without its database, with whether it is.
    pub fn table_items(&self) -> Vec<(String, bool)> {
        match self.tables.as_ref().map(|tables| &tables.state) {
            Some(TableState::Read(items)) => items
                .iter()
                .map(|(selector, on)| {
                    let shown = selector
                        .split_once('.')
                        .map_or(selector.as_str(), |(_, rest)| rest);
                    (shown.to_string(), *on)
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn checked_tables(&self) -> Vec<String> {
        match self.tables.as_ref().map(|tables| &tables.state) {
            Some(TableState::Read(items)) => items
                .iter()
                .filter(|(_, on)| *on)
                .map(|(selector, _)| selector.clone())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The connections MCP never writes to, each with why, left out of the choices.
    pub fn left_out(&self) -> Vec<(String, String)> {
        self.listed()
            .into_iter()
            .filter_map(|choice| choice.refused.map(|why| (choice.name, why)))
            .collect()
    }

    pub fn profile_name(&self) -> String {
        self.profiles
            .get(self.profile)
            .map(|profile| profile.name.clone())
            .unwrap_or_default()
    }

    /// The connections the profile may use, or every saved one when it lists none.
    fn listed(&self) -> Vec<ConnectionChoice> {
        let listed = self
            .profiles
            .get(self.profile)
            .map(|profile| profile.connections.as_slice())
            .unwrap_or_default();
        if listed.is_empty() {
            return self.saved.clone();
        }
        listed
            .iter()
            .map(|name| {
                self.saved
                    .iter()
                    .find(|saved| &saved.name == name)
                    .cloned()
                    .unwrap_or_else(|| ConnectionChoice {
                        name: name.clone(),
                        ..ConnectionChoice::default()
                    })
            })
            .collect()
    }

    /// The connections a grant can be for: the profile's, less those MCP never writes to.
    pub fn connections(&self) -> Vec<ConnectionChoice> {
        self.listed()
            .into_iter()
            .filter(|choice| choice.refused.is_none())
            .collect()
    }

    pub fn connection_choice(&self) -> Option<ConnectionChoice> {
        self.connections().get(self.connection).cloned()
    }

    /// The rows walked, in order: the tables to check only for some, the approval
    /// timeout only while the grant asks.
    pub fn rows(&self) -> Vec<GrantRow> {
        use GrantRow::*;
        let mut rows = vec![Profile, Connection, Tools, Tables];
        if !self.everything {
            rows.push(TablePick);
        }
        rows.extend([Expires, Ask]);
        if self.ask {
            rows.push(AskSecs);
        }
        rows.extend([Confirm, Cancel, Create]);
        rows
    }

    /// Where `row` is among the rows walked: what a click on it is told by.
    pub fn index_of(&self, row: GrantRow) -> usize {
        self.rows().iter().position(|at| *at == row).unwrap_or(0)
    }

    /// Every tool a grant can allow, grouped by what each group does, each with
    /// whether it is checked. The boxes count across the groups.
    pub fn tool_groups(&self) -> Vec<(String, Vec<(String, bool)>)> {
        let mut checked = self.tools.iter();
        CAPABILITIES
            .iter()
            .map(|(name, about, tools)| {
                let items = tools
                    .iter()
                    .map(|tool| (tool.to_string(), *checked.next().unwrap_or(&false)))
                    .collect();
                (format!("{about} ({name})"), items)
            })
            .collect()
    }

    /// The tools checked of each capability that has any: one grant each.
    fn chosen(&self) -> Vec<(&'static str, Vec<String>)> {
        let mut checked = self.tools.iter();
        CAPABILITIES
            .iter()
            .filter_map(|(name, _, tools)| {
                let picked: Vec<String> = tools
                    .iter()
                    .filter(|_| *checked.next().unwrap_or(&false))
                    .map(|tool| tool.to_string())
                    .collect();
                (!picked.is_empty()).then_some((*name, picked))
            })
            .collect()
    }

    /// How many grants Create makes: one for each kind of tool checked.
    pub fn grants(&self) -> usize {
        self.chosen().len()
    }

    /// The tools grid's rows, as drawn.
    fn tool_rows() -> Vec<Vec<usize>> {
        crate::widgets::panel::grid_rows(
            &CAPABILITIES
                .iter()
                .map(|(.., tools)| tools.len())
                .collect::<Vec<_>>(),
        )
    }

    /// The grid the focus is on, and its rows.
    fn grid(&self) -> Option<(usize, Vec<Vec<usize>>)> {
        match self.focus {
            GrantRow::Tools => Some((self.tool, Self::tool_rows())),
            GrantRow::TablePick => Some((
                self.table,
                crate::widgets::panel::grid_rows(&[self.table_items().len()]),
            )),
            _ => None,
        }
    }

    pub fn expires_options() -> Vec<&'static str> {
        EXPIRES.iter().map(|(_, shown, _)| *shown).collect()
    }

    /// The two ways to say which tables: everything the profile sees, or some.
    pub fn tables_options(&self) -> Vec<String> {
        vec![
            format!("Everything {} sees", self.profile_name()),
            "Only some…".into(),
        ]
    }

    /// The grant does not expire: it lasts until it is revoked.
    pub fn until_revoked(&self) -> bool {
        EXPIRES[self.expires].0 == "never"
    }

    /// When it ends, as the end of a sentence.
    fn ends(&self) -> String {
        match EXPIRES[self.expires] {
            ("never", ..) => "lasts until it is revoked".into(),
            (.., long) => format!("ends in {long}"),
        }
    }

    /// How each write goes, in words.
    pub fn how(&self) -> String {
        if self.ask {
            format!("each write waits for you, up to {} s", self.ask_secs.trim())
        } else {
            "one write, then it is spent".into()
        }
    }

    /// What the grants will do, in a sentence, then how long they last.
    pub fn result(&self) -> (Option<String>, String) {
        let chosen = self.chosen();
        let allows = if chosen.is_empty() {
            None
        } else {
            let tables: Vec<String> = if self.everything {
                self.sees()
            } else {
                self.table_items()
                    .into_iter()
                    .filter(|(_, on)| *on)
                    .map(|(name, _)| name)
                    .collect()
            };
            let tables = if tables.is_empty() {
                "…".to_string()
            } else {
                tables.join(", ")
            };
            let connection = self
                .connection_choice()
                .map(|choice| choice.name)
                .unwrap_or_else(|| "…".into());
            // One grant for each kind of tool checked.
            let tools = chosen
                .iter()
                .map(|(_, tools)| tools.join(", "))
                .collect::<Vec<_>>()
                .join(" · ");
            let grants = match chosen.len() {
                1 => String::new(),
                count => format!("{count} grants: "),
            };
            Some(format!("{grants}{tools} on {tables} in {connection}"))
        };
        (allows, format!("{} · {}", self.how(), self.ends()))
    }

    fn step(&mut self, row: GrantRow, delta: isize) {
        let wrap =
            |at: usize, len: usize| (at as isize + delta).rem_euclid(len.max(1) as isize) as usize;
        match row {
            GrantRow::Profile => self.set_profile(wrap(self.profile, self.profiles.len())),
            GrantRow::Connection => {
                self.connection = wrap(self.connection, self.connections().len());
            }
            GrantRow::Tables => self.everything = !self.everything,
            GrantRow::Expires => self.expires = wrap(self.expires, EXPIRES.len()),
            _ => {}
        }
    }

    /// Picks option `option` of `row`: a dropdown's row, a choice, a tool (flipped), or
    /// the toggle.
    pub fn pick(&mut self, row: GrantRow, option: usize) {
        self.error = None;
        self.focus = row;
        match row {
            GrantRow::Profile => {
                self.set_profile(option);
                self.open = None;
            }
            GrantRow::Connection => {
                self.connection = option.min(self.connections().len().saturating_sub(1));
                self.open = None;
            }
            GrantRow::Tables => self.everything = option == 0,
            GrantRow::TablePick => {
                self.table = option;
                if let Some(TableList {
                    state: TableState::Read(items),
                    ..
                }) = self.tables.as_mut()
                    && let Some((_, on)) = items.get_mut(option)
                {
                    *on = !*on;
                }
            }
            GrantRow::Expires => self.expires = option.min(EXPIRES.len() - 1),
            GrantRow::Tools => {
                self.tool = option;
                if let Some(on) = self.tools.get_mut(option) {
                    *on = !*on;
                }
            }
            GrantRow::Ask => self.ask = !self.ask,
            _ => {}
        }
    }

    /// Opens the focused dropdown, on its value.
    pub fn open_dropdown(&mut self) {
        self.open = match self.focus {
            GrantRow::Profile => Some(self.profile),
            GrantRow::Connection if !self.connections().is_empty() => Some(self.connection),
            _ => None,
        };
    }

    /// A key on the form. Enter creates only on Create; on a row it opens, toggles or
    /// moves on.
    pub fn key(&mut self, key: crossterm::event::KeyEvent) -> FormKey {
        use crossterm::event::KeyCode;
        let rows = self.rows();
        let at = rows.iter().position(|row| *row == self.focus).unwrap_or(0);
        let next = rows[(at + 1) % rows.len()];
        let previous = rows[(at + rows.len() - 1) % rows.len()];
        if let Some(highlight) = self.open {
            let last = match self.focus {
                GrantRow::Profile => self.profiles.len(),
                _ => self.connections().len(),
            }
            .saturating_sub(1);
            match key.code {
                KeyCode::Up => self.open = Some(highlight.saturating_sub(1)),
                KeyCode::Down => self.open = Some((highlight + 1).min(last)),
                KeyCode::Enter | KeyCode::Char(' ') => self.pick(self.focus, highlight),
                KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => self.open = None,
                _ => {}
            }
            return FormKey::Stay;
        }
        // The tools and the tables are grids of two columns: Up and Down move between
        // their rows, across the tools' groups, and leave past the first or the last.
        if let Some((cursor, rows)) = self.grid() {
            let last = rows.iter().flatten().count().saturating_sub(1);
            let cursor = cursor.min(last);
            let moved = match key.code {
                KeyCode::Down => crate::widgets::panel::grid_step(&rows, cursor, false),
                KeyCode::Up => crate::widgets::panel::grid_step(&rows, cursor, true),
                KeyCode::Left => Some(cursor.saturating_sub(1)),
                KeyCode::Right => Some((cursor + 1).min(last)),
                _ => None,
            };
            if let Some(moved) = moved {
                self.set_cursor(moved);
                return FormKey::Stay;
            }
        }
        match key.code {
            KeyCode::Tab | KeyCode::Down => {
                self.focus = next;
                // Into a grid from above: its first row, in the same column.
                self.enter_grid(false);
                return FormKey::Stay;
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.focus = previous;
                // Into a grid from below with Up: its last row, in the same column.
                self.enter_grid(key.code == KeyCode::Up);
                return FormKey::Stay;
            }
            KeyCode::Esc => return FormKey::Cancel,
            _ => {}
        }
        match self.focus {
            GrantRow::Create | GrantRow::Cancel => match key.code {
                KeyCode::Left | KeyCode::Right => {
                    self.focus = if self.focus == GrantRow::Create {
                        GrantRow::Cancel
                    } else {
                        GrantRow::Create
                    };
                }
                // The only Enter that creates.
                KeyCode::Enter if self.focus == GrantRow::Create => return FormKey::Submit,
                KeyCode::Enter => return FormKey::Cancel,
                _ => {}
            },
            row => {
                self.error = None;
                match (row, key.code) {
                    (
                        GrantRow::Profile | GrantRow::Connection,
                        KeyCode::Enter | KeyCode::Char(' '),
                    ) => {
                        self.open_dropdown();
                    }
                    (
                        GrantRow::Profile
                        | GrantRow::Connection
                        | GrantRow::Tables
                        | GrantRow::Expires,
                        KeyCode::Left,
                    ) => self.step(row, -1),
                    (
                        GrantRow::Profile
                        | GrantRow::Connection
                        | GrantRow::Tables
                        | GrantRow::Expires,
                        KeyCode::Right,
                    )
                    | (GrantRow::Tables | GrantRow::Expires, KeyCode::Char(' ')) => {
                        self.step(row, 1);
                    }
                    (GrantRow::Tools, KeyCode::Char(' ') | KeyCode::Enter) => {
                        self.pick(
                            GrantRow::Tools,
                            self.tool.min(self.tools.len().saturating_sub(1)),
                        );
                    }
                    (GrantRow::TablePick, KeyCode::Char(' ') | KeyCode::Enter) => {
                        self.pick(GrantRow::TablePick, self.table);
                    }
                    (GrantRow::Ask, KeyCode::Char(' ') | KeyCode::Enter) => self.ask = !self.ask,
                    (_, KeyCode::Enter) => self.focus = next,
                    (GrantRow::AskSecs, _) => {
                        self.ask_secs.handle_key(key);
                    }
                    (GrantRow::Confirm, _) => {
                        self.confirm.handle_key(key);
                    }
                    _ => {}
                }
            }
        }
        FormKey::Stay
    }

    /// Moves the focused grid's cursor.
    fn set_cursor(&mut self, at: usize) {
        match self.focus {
            GrantRow::Tools => self.tool = at,
            GrantRow::TablePick => self.table = at,
            _ => {}
        }
    }

    /// Where a grid's cursor lands as the focus comes into it: its first row, or its last
    /// one coming from below, in the column it was in.
    fn enter_grid(&mut self, from_below: bool) {
        if let Some((cursor, rows)) = self.grid() {
            let at = crate::widgets::panel::grid_enter(&rows, cursor, from_below);
            self.set_cursor(at);
        }
    }

    /// The input the focus is on, for a paste.
    pub fn focused_input(&mut self) -> Option<&mut TextInput> {
        match self.focus {
            GrantRow::AskSecs => Some(&mut self.ask_secs),
            GrantRow::Confirm => Some(&mut self.confirm),
            _ => None,
        }
    }

    /// What the keys do on the focused row.
    pub fn hints(&self) -> &'static [(&'static str, &'static str)] {
        if self.open.is_some() {
            return &[("↑/↓", "choose"), ("Enter", "pick"), ("Esc", "close")];
        }
        match self.focus {
            GrantRow::Profile | GrantRow::Connection => &[
                ("Enter", "open"),
                ("←/→", "change"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            GrantRow::Tables | GrantRow::Expires => {
                &[("←/→", "choose"), ("Tab", "next"), ("Esc", "cancel")]
            }
            GrantRow::Tools | GrantRow::TablePick => &[
                ("↑/↓/←/→", "move"),
                ("Space/Enter", "check"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            GrantRow::Ask => &[
                ("Space/Enter", "toggle"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            GrantRow::AskSecs | GrantRow::Confirm => &[
                ("Enter/Tab", "next"),
                ("Shift+Tab", "back"),
                ("Esc", "cancel"),
            ],
            GrantRow::Create => &[("Enter", "create"), ("←/→", "switch"), ("Esc", "cancel")],
            GrantRow::Cancel => &[("Enter", "cancel"), ("←/→", "switch")],
        }
    }

    /// Puts the focus on the row a refusal is about, so the fix is where the cursor is.
    pub fn fail(&mut self, message: String) {
        let lower = message.to_lowercase();
        let row = [
            ("tool", GrantRow::Tools),
            ("capability", GrantRow::Tools),
            ("a time", GrantRow::Expires),
            ("lasts from", GrantRow::Expires),
            ("confirm", GrantRow::Confirm),
            ("tables", GrantRow::Tables),
            ("sees nothing", GrantRow::Tables),
            ("selector", GrantRow::Tables),
            ("objects", GrantRow::Tables),
            ("profile allows", GrantRow::Tables),
            ("narrow the profile", GrantRow::Tables),
            ("approval timeout", GrantRow::AskSecs),
            ("connection", GrantRow::Connection),
            ("profile", GrantRow::Profile),
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

    /// The requests the CLI would make with the same answers, one for each kind of tool
    /// checked, or what stops them.
    pub fn requests(&self) -> Result<Vec<dexo_app::mcp::GrantRequest>, String> {
        if self.profiles.is_empty() {
            return Err(
                "there is no profile to grant to: create one with dexo mcp profile create".into(),
            );
        }
        let Some(connection) = self.connection_choice() else {
            return Err(
                "connection: none this profile may use takes writes: MCP never writes to production or read-only connections"
                    .into(),
            );
        };
        let chosen = self.chosen();
        if chosen.is_empty() {
            return Err("tools: pick at least one".into());
        }
        let selectors = if self.everything {
            let sees = self.sees();
            if sees.is_empty() {
                let name = self.profile_name();
                return Err(format!(
                    "{name} sees nothing yet: let it see some tables with dexo mcp allow --profile {name} --selector db.public.*"
                ));
            }
            sees
        } else {
            let checked = self.checked_tables();
            if checked.is_empty() {
                return Err("tables: check at least one".into());
            }
            checked
        };
        let ask_secs = if self.ask {
            match self.ask_secs.trim().parse::<u32>() {
                Ok(secs) if (1..=dexo_app::mcp::approval::MAX_TIMEOUT_SECS).contains(&secs) => {
                    Some(secs)
                }
                _ => {
                    return Err(
                        "approval timeout: a whole number of seconds, from 1 to 3600".into(),
                    );
                }
            }
        } else {
            None
        };
        let confirm = self.confirm.trim().to_string();
        if confirm != connection.name {
            return Err(format!("confirm: type {} to confirm", connection.name));
        }
        Ok(chosen
            .into_iter()
            .map(|(capability, tools)| dexo_app::mcp::GrantRequest {
                connection: connection.name.clone(),
                capability: capability.to_string(),
                tools,
                selectors: selectors.clone(),
                expires: EXPIRES[self.expires].0.to_string(),
                confirm_target: confirm.clone(),
                ask_secs,
            })
            .collect())
    }
}

/// What a person is asked to confirm in MCP Profiles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum McpConfirmKind {
    Enable(String),
    RevokeProfile { name: String, grants: usize },
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

    pub fn title(&self) -> &'static str {
        match self.kind {
            McpConfirmKind::Enable(_) => "Enable MCP profile",
            McpConfirmKind::RevokeProfile { .. } | McpConfirmKind::RevokeAll { .. } => {
                "Revoke MCP grants"
            }
            McpConfirmKind::DeleteProfile(_) => "Delete MCP profile",
        }
    }

    pub fn submit_label(&self) -> &'static str {
        match self.kind {
            McpConfirmKind::Enable(_) => "Enable",
            McpConfirmKind::RevokeProfile { .. } | McpConfirmKind::RevokeAll { .. } => "Revoke",
            McpConfirmKind::DeleteProfile(_) => "Delete",
        }
    }

    pub fn lines(&self, connections: &[String]) -> Vec<String> {
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut lines = match &self.kind {
            McpConfirmKind::Enable(name) => vec![
                format!("Enable {name}?"),
                "Agents that connect through it can then use what it allows.".into(),
                if connections.is_empty() {
                    "It lists no connection, so it may use any of yours.".into()
                } else {
                    format!("Connections: {}.", connections.join(", "))
                },
            ],
            McpConfirmKind::RevokeProfile { name, grants } => vec![
                format!(
                    "Revoke the {} of {name}?",
                    plural(*grants, "grant", "grants")
                ),
                "Agents lose the writes they allow; a write waiting for approval is refused."
                    .into(),
            ],
            McpConfirmKind::RevokeAll { profiles, grants } => vec![
                format!(
                    "Revoke every grant: {} in {}?",
                    plural(*grants, "grant", "grants"),
                    plural(*profiles, "profile", "profiles")
                ),
                "Agents lose the writes they allow; a write waiting for approval is refused."
                    .into(),
            ],
            McpConfirmKind::DeleteProfile(name) => vec![
                format!("Delete the profile {name}?"),
                "Its grants go with it, and agents using it stop working.".into(),
            ],
        };
        lines.push(String::new());
        lines.push(footer_line(self.submit_label(), self.focus));
        lines
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpProfilesScreen {
    pub name: String,
    pub enabled: bool,
    pub scopes: Vec<String>,
    pub tools: Vec<String>,
    pub resources: Vec<String>,
    pub grants: Vec<GrantLine>,
    pub connections: Vec<String>,
    pub raw_read: bool,
    /// What the last action did, until the selection moves or the screen is opened again.
    pub status: String,
    pub profiles: Vec<McpProfileSummary>,
    pub selected: usize,
    /// Lines the picked profile's details are scrolled down.
    pub detail_scroll: usize,
    pub confirm: Option<McpConfirm>,
    /// `g`: a new grant, being filled in.
    pub grant_form: Option<GrantForm>,
    /// The grant form was asked for before the profiles were read.
    pub grant_when_loaded: bool,
    /// "Revoke all" was asked for before the profiles were read: it names how many go.
    pub revoke_all_when_loaded: bool,
    /// `c`: the connections the picked profile may use, being checked.
    pub checklist: Option<Checklist>,
}

/// Where the focus is in the connections form.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChecklistFocus {
    #[default]
    Grid,
    Cancel,
    Save,
}

/// Connections to check, the box the grid's cursor is on, and where the focus is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Checklist {
    /// The profile the connections are for.
    pub profile: String,
    pub items: Vec<(String, bool)>,
    pub row: usize,
    pub focus: ChecklistFocus,
    pub error: Option<String>,
}

impl Checklist {
    /// `available` with the profile's `checked`; one it lists that is not available any
    /// more stays, checked, so saving does not drop it unseen.
    pub fn new(profile: &str, available: Vec<String>, checked: &[String]) -> Self {
        let mut items: Vec<(String, bool)> = available
            .into_iter()
            .map(|name| {
                let on = checked.contains(&name);
                (name, on)
            })
            .collect();
        for name in checked {
            if !items.iter().any(|(item, _)| item == name) {
                items.push((name.clone(), true));
            }
        }
        Self {
            profile: profile.to_string(),
            items,
            ..Self::default()
        }
    }

    /// A key on the form: the grid of two columns moves with the arrows and checks with
    /// Space or Enter; Enter saves only on Save.
    pub fn key(&mut self, key: crossterm::event::KeyEvent) -> FormKey {
        use crossterm::event::KeyCode;
        let last = self.items.len().saturating_sub(1);
        let row = self.row.min(last);
        match (self.focus, key.code) {
            (_, KeyCode::Esc) => return FormKey::Cancel,
            (ChecklistFocus::Grid, KeyCode::Down) if row / 2 < last / 2 => {
                self.row = (row + 2).min(last);
            }
            (ChecklistFocus::Grid, KeyCode::Up) if row / 2 > 0 => self.row = row - 2,
            (ChecklistFocus::Grid, KeyCode::Left) => self.row = row.saturating_sub(1),
            (ChecklistFocus::Grid, KeyCode::Right) => self.row = (row + 1).min(last),
            (ChecklistFocus::Grid, KeyCode::Char(' ') | KeyCode::Enter) => {
                self.error = None;
                self.toggle(row);
            }
            (ChecklistFocus::Grid, KeyCode::Tab | KeyCode::Down) => {
                self.focus = ChecklistFocus::Cancel;
            }
            (ChecklistFocus::Cancel, KeyCode::Tab | KeyCode::Right | KeyCode::Left) => {
                self.focus = ChecklistFocus::Save;
            }
            (ChecklistFocus::Save, KeyCode::Left | KeyCode::Right | KeyCode::BackTab) => {
                self.focus = ChecklistFocus::Cancel;
            }
            (ChecklistFocus::Save, KeyCode::Tab)
            | (ChecklistFocus::Cancel, KeyCode::BackTab | KeyCode::Up) => {
                self.focus = ChecklistFocus::Grid;
            }
            (ChecklistFocus::Save, KeyCode::Up) => self.focus = ChecklistFocus::Grid,
            (ChecklistFocus::Cancel, KeyCode::Enter) => return FormKey::Cancel,
            (ChecklistFocus::Save, KeyCode::Enter) => return FormKey::Submit,
            _ => {}
        }
        FormKey::Stay
    }

    pub fn hints(&self) -> &'static [(&'static str, &'static str)] {
        match self.focus {
            ChecklistFocus::Grid => &[
                ("↑/↓/←/→", "move"),
                ("Space/Enter", "check"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            ChecklistFocus::Save => &[("Enter", "save"), ("←/→", "switch"), ("Esc", "cancel")],
            ChecklistFocus::Cancel => &[("Enter", "cancel"), ("←/→", "switch")],
        }
    }

    pub fn checked(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|(_, on)| *on)
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn toggle(&mut self, row: usize) {
        if let Some((_, on)) = self.items.get_mut(row) {
            *on = !*on;
        }
        self.row = row.min(self.items.len().saturating_sub(1));
    }

    pub fn step(&mut self, step: isize) {
        let last = self.items.len().saturating_sub(1) as isize;
        self.row = (self.row as isize + step).clamp(0, last) as usize;
    }
}

impl McpProfilesScreen {
    pub fn fixture() -> Self {
        let mut screen = Self {
            profiles: vec![McpProfileSummary {
                name: "assistant".into(),
                enabled: false,
                connections: vec!["local".into()],
                scopes: vec!["allow db.public.*".into(), "deny db.public.secrets".into()],
                tools: vec!["catalog_search".into(), "object_describe".into()],
                grants: vec![GrantLine {
                    id: "g1".into(),
                    capability: "data_write".into(),
                    tools: "data_insert".into(),
                    expires_in_secs: 900,
                    connection: "local".into(),
                    selectors: "db.public.items".into(),
                    ask_secs: 0,
                    until_revoked: false,
                }],
                ..McpProfileSummary::default()
            }],
            ..Self::default()
        };
        screen.apply_selected();
        screen
    }

    pub fn selected_profile(&self) -> Option<&McpProfileSummary> {
        self.profiles.get(self.selected)
    }

    /// Flips the selected profile. Enabling hands an MCP client tool access to the
    /// database, so it asks first; disabling only takes access away and does not.
    /// Returns the new state once it changed.
    pub fn toggle_selected(&mut self) -> Option<bool> {
        if self.name.is_empty() {
            self.status = "Create a profile first: dexo mcp profile create --name NAME.".into();
            return None;
        }
        if self.enabled {
            self.enabled = false;
            self.status = format!("Disabled {}: agents can no longer use it.", self.name);
            return Some(false);
        }
        self.confirm = Some(McpConfirm::new(McpConfirmKind::Enable(self.name.clone())));
        None
    }

    /// Asks to revoke the picked profile's grants, when it has any.
    pub fn ask_revoke_profile(&mut self) {
        if self.name.is_empty() {
            self.status = "Create a profile first: dexo mcp profile create --name NAME.".into();
        } else if self.grants.is_empty() {
            self.status = format!("{} has no grants to revoke.", self.name);
        } else {
            self.confirm = Some(McpConfirm::new(McpConfirmKind::RevokeProfile {
                name: self.name.clone(),
                grants: self.grants.len(),
            }));
        }
    }

    /// Asks to revoke every grant of every profile, when there are any.
    pub fn ask_revoke_all(&mut self) {
        let grants: usize = self
            .profiles
            .iter()
            .map(|profile| profile.grants.len())
            .sum();
        if grants == 0 {
            self.status = "No profile has a grant to revoke.".into();
        } else {
            let profiles = self
                .profiles
                .iter()
                .filter(|profile| !profile.grants.is_empty())
                .count();
            self.confirm = Some(McpConfirm::new(McpConfirmKind::RevokeAll {
                profiles,
                grants,
            }));
        }
    }

    pub fn ask_delete(&mut self) {
        if self.name.is_empty() {
            self.status = "There is no profile to delete.".into();
        } else {
            self.confirm = Some(McpConfirm::new(McpConfirmKind::DeleteProfile(
                self.name.clone(),
            )));
        }
    }

    pub fn tick(&mut self) {
        for grant in &mut self.grants {
            grant.expires_in_secs = grant.expires_in_secs.saturating_sub(1);
        }
    }

    /// Takes the profiles as read again. The pick stays on its profile by name, so what
    /// was just made -- a grant, a change -- is still on screen.
    pub fn load_profiles(&mut self, profiles: Vec<McpProfileSummary>) {
        let keep = self.name.clone();
        self.profiles = profiles;
        self.selected = self
            .profiles
            .iter()
            .position(|profile| profile.name == keep)
            .unwrap_or_else(|| self.selected.min(self.profiles.len().saturating_sub(1)));
        // A confirmation belongs to what it was asked about.
        if self
            .confirm
            .as_ref()
            .is_some_and(|confirm| !self.confirm_still_applies(&confirm.kind))
        {
            self.confirm = None;
        }
        self.apply_selected();
    }

    fn confirm_still_applies(&self, kind: &McpConfirmKind) -> bool {
        match kind {
            McpConfirmKind::Enable(name) | McpConfirmKind::DeleteProfile(name) => {
                self.profiles.iter().any(|profile| &profile.name == name)
            }
            McpConfirmKind::RevokeProfile { name, .. } => self
                .profiles
                .iter()
                .any(|profile| &profile.name == name && !profile.grants.is_empty()),
            McpConfirmKind::RevokeAll { .. } => self.profiles.iter().any(|p| !p.grants.is_empty()),
        }
    }

    pub fn select_previous(&mut self) {
        self.move_selection(-1);
    }

    pub fn select_next(&mut self) {
        self.move_selection(1);
    }

    fn move_selection(&mut self, delta: isize) {
        let last = self.profiles.len().saturating_sub(1);
        let moved = self.selected.saturating_add_signed(delta).min(last);
        if moved != self.selected {
            // What the last action said was about the profile it was done to.
            self.status.clear();
            self.detail_scroll = 0;
        }
        self.selected = moved;
        self.apply_selected();
    }

    pub fn select_index(&mut self, index: usize) {
        let moved = index.min(self.profiles.len().saturating_sub(1));
        if moved != self.selected {
            self.status.clear();
            self.detail_scroll = 0;
        }
        self.selected = moved;
        self.apply_selected();
    }

    fn apply_selected(&mut self) {
        match self.profiles.get(self.selected).cloned() {
            Some(profile) => {
                self.name = profile.name;
                self.enabled = profile.enabled;
                self.scopes = profile.scopes;
                self.tools = profile.tools;
                self.grants = profile.grants;
                self.connections = profile.connections;
                self.raw_read = profile.raw_read;
            }
            None => {
                self.name.clear();
                self.enabled = false;
                self.scopes.clear();
                self.tools.clear();
                self.grants.clear();
                self.connections.clear();
                self.raw_read = false;
            }
        }
    }

    /// The picked profile's details, whole.
    pub fn detail_lines(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut put = |text: String, first: &str, rest: &str| {
            let room = width.saturating_sub(first.chars().count()).max(8);
            for (index, part) in crate::model::wrap_words(&text, room)
                .into_iter()
                .enumerate()
            {
                lines.push(format!("{}{part}", if index == 0 { first } else { rest }));
            }
        };
        put(
            format!(
                "{} is {}.",
                self.name,
                if self.enabled {
                    "enabled: agents can use it"
                } else {
                    "disabled: agents cannot use it yet"
                }
            ),
            "",
            "",
        );
        if self.connections.is_empty() {
            put(
                format!(
                    "Connections: none listed, so any (dexo mcp profile set --name {} --connection NAME)",
                    self.name
                ),
                "",
                "  ",
            );
        } else {
            put(
                format!("Connections: {}", self.connections.join(", ")),
                "",
                "  ",
            );
        }
        put(
            if self.raw_read {
                "Reads: structured tools, and raw read-only SQL (query_execute_read)".into()
            } else {
                "Reads: structured tools only".into()
            },
            "",
            "  ",
        );
        let (mut sees, mut denies) = (Vec::new(), Vec::new());
        for scope in &self.scopes {
            match scope.strip_prefix("deny ") {
                Some(denied) => denies.push(denied.to_string()),
                None => sees.push(scope.strip_prefix("allow ").unwrap_or(scope).to_string()),
            }
        }
        if sees.is_empty() {
            put(
                format!(
                    "Objects: none allowed yet (dexo mcp allow --profile {} --selector db.schema.*)",
                    self.name
                ),
                "",
                "  ",
            );
        } else {
            put(format!("Objects it can see: {}", sees.join(", ")), "", "  ");
        }
        for denied in denies {
            put(format!("never: {denied}"), "  ", "    ");
        }
        if !self.tools.is_empty() {
            put(
                format!("Tools set by hand: {}", self.tools.join(", ")),
                "",
                "  ",
            );
        }
        if self.grants.is_empty() {
            put("No grants: agents cannot write.".into(), "", "");
        } else {
            put(
                format!(
                    "Grants ({}): agents may write while they last",
                    self.grants.len()
                ),
                "",
                "",
            );
            for grant in &self.grants {
                put(grant.words(), "  ", "    ");
            }
        }
        lines
    }

    /// A profile in the list's row, after its glyph: its connections, what it reads, its
    /// grants.
    pub fn columns(profile: &McpProfileSummary) -> String {
        let connections = match profile.connections.len() {
            0 => "any conn".to_string(),
            n => format!("{n} conn"),
        };
        let grants = match profile.grants.len() {
            0 => String::new(),
            1 => "  1 grant".into(),
            n => format!("  {n} grants"),
        };
        format!(
            "{connections:<8}  {:<8}{grants}",
            if profile.raw_read {
                "read SQL"
            } else {
                "browse"
            }
        )
    }

    /// A profile in one row of the list.
    pub fn row(profile: &McpProfileSummary) -> String {
        let state = if profile.enabled {
            "enabled "
        } else {
            "disabled"
        };
        let grants = match profile.grants.len() {
            0 => String::new(),
            1 => "  1 grant".into(),
            n => format!("  {n} grants"),
        };
        format!("{}  {state}{grants}", profile.name)
    }

    /// What the screen says with no profile to show: how to make one.
    pub const EMPTY: [&'static str; 3] = [
        "No MCP profiles yet.",
        "Agents connect through a profile, which says what they may see.",
        "n -- or 4, Setup -- makes one and sets up Claude Code, Codex, Cursor and the rest with it.",
    ];

    /// The list and the picked profile's details as text, the way they read on screen.
    pub fn lines(&self) -> Vec<String> {
        if self.profiles.is_empty() {
            return Self::EMPTY.iter().map(|line| line.to_string()).collect();
        }
        let mut lines: Vec<String> = self.profiles.iter().map(Self::row).collect();
        lines.push(String::new());
        lines.extend(self.detail_lines(100));
        if !self.status.is_empty() {
            lines.push(self.status.clone());
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, grants: usize) -> McpProfileSummary {
        McpProfileSummary {
            name: name.into(),
            connections: vec!["pg-dev".into()],
            scopes: vec![
                "allow qa7.public.*".into(),
                "deny qa7.public.customers".into(),
            ],
            grants: (0..grants)
                .map(|n| GrantLine {
                    id: format!("g{n}"),
                    capability: "data_write".into(),
                    tools: "data_update".into(),
                    expires_in_secs: 1796,
                    connection: "pg-dev".into(),
                    selectors: "qa7.public.orders".into(),
                    ask_secs: if n == 0 { 120 } else { 0 },
                    until_revoked: false,
                })
                .collect(),
            ..McpProfileSummary::default()
        }
    }

    #[test]
    fn enabling_asks_first_and_disabling_does_not() {
        let mut screen = McpProfilesScreen::fixture();
        assert!(!screen.enabled);
        assert_eq!(screen.toggle_selected(), None, "enabling only asks");
        assert!(matches!(
            screen.confirm.as_ref().map(|confirm| &confirm.kind),
            Some(McpConfirmKind::Enable(name)) if name == "assistant"
        ));
        assert_eq!(
            screen.confirm.as_ref().unwrap().focus,
            FooterFocus::Cancel,
            "an Enter out of habit enables nothing"
        );
        screen.enabled = true;
        screen.confirm = None;
        assert_eq!(screen.toggle_selected(), Some(false));
        assert!(screen.status.contains("Disabled assistant"));
    }

    #[test]
    fn the_screen_says_it_in_words_not_in_key_value_dumps() {
        let mut screen = McpProfilesScreen::default();
        screen.load_profiles(vec![profile("pg-dev", 2)]);
        let text = screen.lines().join("\n");
        for raw in ["enabled=", "confirm=", "diff ", "scopes=", "profile pg-dev"] {
            assert!(!text.contains(raw), "{raw}: {text}");
        }
        assert!(text.contains("never: qa7.public.customers"), "{text}");
        assert!(
            text.contains("asks before each write (2 min), ends in 30 min"),
            "{text}"
        );
        assert!(text.contains("one write, then it is spent"), "{text}");
    }

    #[test]
    fn the_status_does_not_follow_the_pick_to_another_profile() {
        let mut screen = McpProfilesScreen::default();
        screen.load_profiles(vec![profile("a", 0), profile("b", 0)]);
        screen.status = "Disabled a: agents can no longer use it.".into();
        screen.load_profiles(vec![profile("a", 0), profile("b", 0)]);
        assert!(!screen.status.is_empty(), "a reload keeps what was said");
        screen.select_next();
        assert!(screen.status.is_empty());
        assert_eq!(screen.name, "b");
    }

    #[test]
    fn an_empty_screen_says_how_to_make_a_profile() {
        let screen = McpProfilesScreen::default();
        let text = screen.lines().join("\n");
        assert!(text.contains("Setup"), "{text}");
    }

    #[test]
    fn revoking_names_what_goes_and_has_nothing_to_ask_when_nothing_is_there() {
        let mut screen = McpProfilesScreen::default();
        screen.load_profiles(vec![profile("a", 0), profile("b", 3)]);
        screen.ask_revoke_profile();
        assert!(screen.confirm.is_none());
        assert!(screen.status.contains("no grants"));
        screen.ask_revoke_all();
        let confirm = screen.confirm.clone().expect("asks");
        let text = confirm.lines(&[]).join("\n");
        assert!(text.contains("3 grants in 1 profile"), "{text}");
        assert!(text.contains("[Revoke]"), "{text}");
    }

    fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    fn form() -> GrantForm {
        let choices = vec![
            ProfileChoice {
                name: "a".into(),
                connections: vec!["pg-dev".into(), "pg-prod".into()],
                sees: vec!["db.public.*".into()],
            },
            ProfileChoice {
                name: "b".into(),
                connections: Vec::new(),
                sees: Vec::new(),
            },
        ];
        let saved = vec![
            ConnectionChoice {
                name: "pg-dev".into(),
                environment: "development".into(),
                refused: None,
                postgres: true,
            },
            ConnectionChoice {
                name: "pg-prod".into(),
                environment: "production".into(),
                refused: Some("MCP never writes to production".into()),
                postgres: true,
            },
        ];
        GrantForm::new(choices, saved, 1)
    }

    #[test]
    fn the_grant_form_picks_its_profile_and_its_connections() {
        use crossterm::event::KeyCode;
        let mut form = form();
        assert_eq!(form.focus, GrantRow::Profile);
        assert_eq!(form.profile_name(), "b");
        // A profile that lists no connection may use any saved one MCP writes to.
        assert_eq!(form.connections().len(), 1);
        form.key(key(KeyCode::Left));
        assert_eq!(form.profile_name(), "a");
        let connections = form.connections();
        assert_eq!(connections.len(), 1, "production is left out");
        assert_eq!(connections[0].environment, "development");
        assert_eq!(
            form.left_out(),
            [(
                "pg-prod".to_string(),
                "MCP never writes to production".to_string()
            )]
        );
        // Enter opens the dropdown, and picks in it.
        form.key(key(KeyCode::Enter));
        assert_eq!(form.open, Some(0));
        form.key(key(KeyCode::Down));
        form.key(key(KeyCode::Enter));
        assert_eq!((form.profile_name().as_str(), form.open), ("b", None));
    }

    /// Everything the profile sees, with nothing to type; or only the tables checked,
    /// read from the connection's catalog and kept inside what the profile sees.
    #[test]
    fn the_tables_are_everything_it_sees_or_the_ones_checked() {
        use crossterm::event::KeyCode;
        let mut form = form();
        form.key(key(KeyCode::Left));
        form.pick(GrantRow::Tools, 0);
        form.confirm.set_text("pg-dev");
        let request = form.requests().unwrap().remove(0);
        assert_eq!(request.selectors, ["db.public.*"]);
        assert_eq!(form.wants_tables(), None);
        // Only some: the connection's tables are read once.
        form.focus = GrantRow::Tables;
        form.key(key(KeyCode::Right));
        assert!(!form.everything && form.rows().contains(&GrantRow::TablePick));
        assert_eq!(form.wants_tables(), Some(("pg-dev".to_string(), true)));
        form.tables = Some(TableList {
            connection: "pg-dev".into(),
            state: TableState::Reading,
        });
        form.take_tables(
            "pg-dev",
            Some(vec![
                "db.public.orders".into(),
                "db.public.items".into(),
                "db.audit.log".into(),
            ]),
        );
        assert_eq!(form.wants_tables(), None);
        let names: Vec<String> = form
            .table_items()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            names,
            ["public.orders", "public.items"],
            "outside what it sees: gone"
        );
        assert_eq!(form.requests().unwrap_err(), "tables: check at least one");
        form.pick(GrantRow::TablePick, 1);
        assert_eq!(form.requests().unwrap()[0].selectors, ["db.public.items"]);
        // A profile that sees nothing says how to fix it.
        form.pick(GrantRow::Profile, 1);
        form.everything = true;
        assert!(form.requests().unwrap_err().contains("b sees nothing yet"));
    }

    #[test]
    fn a_grant_can_last_until_it_is_revoked() {
        use crossterm::event::KeyCode;
        let mut form = form();
        form.focus = GrantRow::Expires;
        form.key(key(KeyCode::Left));
        assert!(form.until_revoked());
        assert!(
            form.result().1.ends_with("until it is revoked"),
            "{:?}",
            form.result()
        );
        form.key(key(KeyCode::Left));
        form.pick(GrantRow::Profile, 0);
        form.pick(GrantRow::Tools, 0);
        form.confirm.set_text("pg-dev");
        form.expires = 5;
        assert_eq!(form.requests().unwrap()[0].expires, "never");
    }

    #[test]
    fn enter_creates_only_on_create() {
        use crossterm::event::KeyCode;
        let mut form = form();
        form.pick(GrantRow::Profile, 0);
        // Each row: Enter opens, toggles or moves on, never submits.
        form.focus = GrantRow::Tools;
        assert_eq!(form.key(key(KeyCode::Enter)), FormKey::Stay);
        assert!(form.tools[0], "Enter checks the tool");
        form.focus = GrantRow::Ask;
        assert_eq!(form.key(key(KeyCode::Enter)), FormKey::Stay);
        assert!(form.ask, "Enter flips the toggle");
        assert!(form.rows().contains(&GrantRow::AskSecs));
        form.focus = GrantRow::Confirm;
        for ch in "pg-dev".chars() {
            form.key(key(KeyCode::Char(ch)));
        }
        assert_eq!(form.key(key(KeyCode::Enter)), FormKey::Stay);
        assert_eq!(form.focus, GrantRow::Cancel);
        assert_eq!(form.key(key(KeyCode::Enter)), FormKey::Cancel);
        form.focus = GrantRow::Create;
        assert_eq!(form.key(key(KeyCode::Enter)), FormKey::Submit);
        let request = form.requests().unwrap().remove(0);
        assert_eq!(request.tools, ["data_insert"]);
        assert_eq!(
            (request.selectors.as_slice(), request.expires.as_str()),
            (["db.public.*".to_string()].as_slice(), "15m")
        );
        assert_eq!(request.ask_secs, Some(120));
    }

    /// Every tool in one grid, grouped by what it does: Up and Down walk its rows across
    /// the groups, and leave it past the first or the last.
    #[test]
    fn the_tools_are_one_grid_the_arrows_walk() {
        use crossterm::event::KeyCode;
        let mut form = form();
        form.focus = GrantRow::Connection;
        form.key(key(KeyCode::Down));
        assert_eq!((form.focus, form.tool), (GrantRow::Tools, 0));
        // data_write is two rows; ddl one, of one tool; admin one, of two.
        let mut walked = Vec::new();
        form.key(key(KeyCode::Right));
        for _ in 0..3 {
            form.key(key(KeyCode::Down));
            walked.push(form.tool);
        }
        assert_eq!(walked, [3, 4, 5]);
        form.key(key(KeyCode::Down));
        assert_eq!(form.focus, GrantRow::Tables);
        form.key(key(KeyCode::Up));
        assert_eq!((form.focus, form.tool), (GrantRow::Tools, 5));
        for _ in 0..4 {
            form.key(key(KeyCode::Up));
        }
        assert_eq!(form.focus, GrantRow::Connection);
    }

    /// Tools of two kinds checked make two grants, with the same tables and time.
    #[test]
    fn tools_of_two_kinds_make_two_grants() {
        let mut form = form();
        form.pick(GrantRow::Profile, 0);
        form.pick(GrantRow::Tools, 0);
        form.pick(GrantRow::Tools, 4);
        form.confirm.set_text("pg-dev");
        let requests = form.requests().unwrap();
        let kinds: Vec<(&str, Vec<String>)> = requests
            .iter()
            .map(|request| (request.capability.as_str(), request.tools.clone()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("data_write", vec!["data_insert".to_string()]),
                ("ddl", vec!["schema_apply_ddl".to_string()])
            ]
        );
        assert!(
            requests
                .iter()
                .all(|request| request.selectors == ["db.public.*"])
        );
        let (allows, _) = form.result();
        assert!(
            allows.is_some_and(
                |allows| allows.starts_with("2 grants: data_insert · schema_apply_ddl")
            ),
            "{:?}",
            form.result()
        );
    }

    /// A refusal puts the cursor on the row it is about, and a change takes it away.
    #[test]
    fn a_refusal_focuses_its_row() {
        use crossterm::event::KeyCode;
        let mut form = form();
        form.fail("a grant lasts from 1 second to 24 hours: write the time as 90s".into());
        assert_eq!(form.focus, GrantRow::Expires);
        form.fail(
            "db.public.* is outside what the profile allows: a grant can only narrow the profile"
                .into(),
        );
        assert_eq!(form.focus, GrantRow::Tables);
        assert_eq!(
            form.requests().unwrap_err(),
            "tools: pick at least one",
            "the tools are picked on purpose"
        );
        form.fail("confirm: type pg-dev to confirm".into());
        assert_eq!(form.focus, GrantRow::Confirm);
        form.key(key(KeyCode::Char('p')));
        assert!(form.error.is_none(), "a change takes the old refusal away");
    }
}
