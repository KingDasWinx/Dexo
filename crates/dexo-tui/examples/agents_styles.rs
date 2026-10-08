//! Agents → Profiles drawn three ways on the new model -- one write access per
//! connection, every write asking first -- to choose one before it is built:
//!
//!     cargo run -p dexo-tui --example agents_styles
//!
//! F1 Sheet puts all of a profile on one page, its connections a table to walk. F2 Tabs is
//! lazydb's principal view: a summary over a Connections tab and an Agents tab. F3 Columns
//! is lazygit's panels: profiles, the picked one's connections and agents, then the picked
//! row. n, Enter, w and a open the forms of `docs/design.md` where the detail was. Nothing
//! is saved: the forms change only this example's profiles.

use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use dexo_tui::capabilities::TerminalCapabilities;
use dexo_tui::model::Screen;
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::screen::Section;
use dexo_tui::screen::widgets::{self, Button, Entry};
use dexo_tui::theme::{DEFAULT_ACCENT, Mode, Role, theme_for};
use dexo_tui::widgets::panel::{self, Lines};
use dexo_tui::widgets::text_input::TextInput;
use dexo_tui::{Action, Model, update};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

struct Conn {
    name: &'static str,
    env: &'static str,
    tables: &'static [&'static str],
}

impl Conn {
    fn writable(&self) -> bool {
        self.env != "production"
    }
}

const CONNS: [Conn; 4] = [
    Conn {
        name: "pg-dev",
        env: "development",
        tables: &[
            "app.public.orders",
            "app.public.items",
            "app.public.customers",
            "app.public.secrets",
            "app.billing.invoices",
        ],
    },
    Conn {
        name: "my-dev",
        env: "development",
        tables: &["shop.products", "shop.carts", "shop.users"],
    },
    Conn {
        name: "pg-staging",
        env: "staging",
        tables: &["app.public.orders", "app.public.items"],
    },
    Conn {
        name: "pg-prod",
        env: "production",
        tables: &["app.public.orders", "app.public.customers"],
    },
];

struct Client {
    name: &'static str,
    file: &'static str,
    found: bool,
}

const CLIENTS: [Client; 7] = [
    Client {
        name: "Claude Code",
        file: ".mcp.json in this folder",
        found: true,
    },
    Client {
        name: "Codex",
        file: "~/.codex/config.toml",
        found: true,
    },
    Client {
        name: "Cursor",
        file: "~/.cursor/mcp.json",
        found: false,
    },
    Client {
        name: "Claude Desktop",
        file: "~/.config/Claude/claude_desktop_config.json",
        found: false,
    },
    Client {
        name: "Gemini CLI",
        file: "~/.gemini/settings.json",
        found: false,
    },
    Client {
        name: "Windsurf",
        file: "~/.codeium/windsurf/mcp_config.json",
        found: false,
    },
    Client {
        name: "VS Code",
        file: ".vscode/mcp.json in this folder",
        found: false,
    },
];

/// What a write access may change: the box, and what it covers.
const KINDS: [(&str, &str); 3] = [
    ("change rows", "insert, update, delete, and SQL that writes"),
    ("change schema", "CREATE, ALTER, DROP, indexes, views"),
    ("end sessions", "cancel a query, end a server session"),
];
const KIND_WORDS: [&str; 3] = ["rows", "schema", "sessions"];

const LASTS: [(&str, &str); 6] = [
    ("15m", "for 15 minutes"),
    ("1h", "for 1 hour"),
    ("2h", "for 2 hours"),
    ("8h", "for 8 hours"),
    ("24h", "for 24 hours"),
    ("Until revoked", "until you revoke it"),
];

#[derive(Clone)]
struct Access {
    kinds: [bool; 3],
    lasts: usize,
    left: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sees {
    All,
    Except,
    Only,
}

#[derive(Clone)]
struct Profile {
    name: String,
    enabled: bool,
    /// Each connection, by its index in CONNS, with its write access.
    conns: Vec<(usize, Option<Access>)>,
    sql: bool,
    sees: Sees,
    /// The tables hidden (Except) or the only ones seen (Only).
    tables: Vec<String>,
    /// The agents started with it, by their index in CLIENTS.
    agents: Vec<usize>,
}

fn kinds_words(kinds: &[bool; 3]) -> String {
    KIND_WORDS
        .iter()
        .zip(kinds)
        .filter(|(_, on)| **on)
        .map(|(word, _)| *word)
        .collect::<Vec<_>>()
        .join(", ")
}

fn reads_words(profile: &Profile) -> &'static str {
    if profile.sql {
        "tables and SELECT, 1000 rows at most"
    } else {
        "tables only: browse, describe, read pages"
    }
}

fn sees_words(sees: Sees, tables: &[String]) -> String {
    match sees {
        Sees::All => "everything in its connections".into(),
        Sees::Except => format!("everything, except {}", tables.join(", ")),
        Sees::Only => format!("only {}", tables.join(", ")),
    }
}

fn agents_words(profile: &Profile) -> String {
    match profile.agents.len() {
        0 => "no agent".into(),
        1 => CLIENTS[profile.agents[0]].name.into(),
        n => format!("{n} agents"),
    }
}

/// A connection's writes in words, and their colour.
fn writes_words(conn: &Conn, access: &Option<Access>, short: bool) -> (String, Role) {
    match access {
        Some(access) if short => (
            format!("✎ {}", kinds_words(&access.kinds)),
            Role::Foreground,
        ),
        Some(access) => (
            format!("✎ {} · {}", kinds_words(&access.kinds), access.left),
            Role::Foreground,
        ),
        None if !conn.writable() && !short => ("read only: production".into(), Role::Muted),
        None => ("read only".into(), Role::Muted),
    }
}

fn left_after(lasts: usize) -> String {
    match lasts {
        0 => "15 min left".into(),
        1 => "1 h left".into(),
        2 => "2 h left".into(),
        3 => "8 h left".into(),
        4 => "24 h left".into(),
        _ => "until revoked".into(),
    }
}

fn profiles() -> Vec<Profile> {
    vec![
        Profile {
            name: "assistant".into(),
            enabled: true,
            conns: vec![
                (
                    0,
                    Some(Access {
                        kinds: [true, true, false],
                        lasts: 1,
                        left: "28 min left".into(),
                    }),
                ),
                (1, None),
            ],
            sql: true,
            sees: Sees::Except,
            tables: vec!["app.public.secrets".into()],
            agents: vec![0, 1],
        },
        Profile {
            name: "reporting".into(),
            enabled: false,
            conns: vec![(2, None), (3, None)],
            sql: false,
            sees: Sees::Only,
            tables: vec!["app.public.orders".into(), "app.public.items".into()],
            agents: Vec::new(),
        },
    ]
}

// -- The forms, built with Dexo's own panel ------------------------------------------

enum Outcome {
    Stay,
    Cancel,
    Submit,
}

/// The row `delta` places from `at`, wrapping.
fn walk<R: Copy + PartialEq>(rows: &[R], at: R, delta: isize) -> R {
    let index = rows.iter().position(|row| *row == at).unwrap_or(0) as isize;
    rows[(index + delta).rem_euclid(rows.len() as isize) as usize]
}

/// The arrows on a grid of `sizes` boxes: true while they move inside it.
fn grid_key(sizes: &[usize], cursor: &mut usize, code: KeyCode) -> bool {
    let rows = panel::grid_rows(sizes);
    let last = sizes.iter().sum::<usize>().saturating_sub(1);
    let moved = match code {
        KeyCode::Down => panel::grid_step(&rows, *cursor, false),
        KeyCode::Up => panel::grid_step(&rows, *cursor, true),
        KeyCode::Left => Some(cursor.saturating_sub(1)),
        KeyCode::Right => Some((*cursor + 1).min(last)),
        _ => None,
    };
    match moved {
        Some(at) => {
            *cursor = at;
            true
        }
        None => false,
    }
}

fn buttons<'a>(cancel: bool, submit: bool, label: &'a str) -> [panel::Button<'a>; 2] {
    [
        panel::Button {
            label: "Cancel",
            focused: cancel,
            tone: Role::Focus,
            primary: false,
            target: HitTarget::FooterCancel,
        },
        panel::Button {
            label,
            focused: submit,
            tone: Role::Focus,
            primary: true,
            target: HitTarget::FooterSubmit,
        },
    ]
}

/// Up, Down, Tab and Shift+Tab between rows; Left and Right between the buttons; Enter
/// on them; Esc anywhere. `None`: the key is the row's.
fn form_key<R: Copy + PartialEq>(
    rows: &[R],
    focus: &mut R,
    cancel: R,
    submit: R,
    key: KeyEvent,
) -> Option<Outcome> {
    match key.code {
        KeyCode::Esc => return Some(Outcome::Cancel),
        KeyCode::Tab | KeyCode::Down => *focus = walk(rows, *focus, 1),
        KeyCode::BackTab | KeyCode::Up => *focus = walk(rows, *focus, -1),
        KeyCode::Left | KeyCode::Right if *focus == cancel || *focus == submit => {
            *focus = if *focus == cancel { submit } else { cancel };
        }
        KeyCode::Enter if *focus == submit => return Some(Outcome::Submit),
        KeyCode::Enter if *focus == cancel => return Some(Outcome::Cancel),
        _ => return None,
    }
    Some(Outcome::Stay)
}

fn dropdown_key(open: &mut Option<usize>, chosen: &mut usize, count: usize, code: KeyCode) -> bool {
    let last = count.saturating_sub(1);
    match (*open, code) {
        (Some(at), KeyCode::Up) => *open = Some(at.saturating_sub(1)),
        (Some(at), KeyCode::Down) => *open = Some((at + 1).min(last)),
        (Some(at), KeyCode::Enter | KeyCode::Char(' ')) => {
            *chosen = at;
            *open = None;
        }
        (Some(_), KeyCode::Esc | KeyCode::Tab) => *open = None,
        (Some(_), _) => {}
        (None, KeyCode::Enter | KeyCode::Char(' ')) => *open = Some(*chosen),
        (None, KeyCode::Left) => *chosen = chosen.saturating_sub(1),
        (None, KeyCode::Right) => *chosen = (*chosen + 1).min(last),
        (None, _) => return false,
    }
    true
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PRow {
    Name,
    Conns,
    Reads,
    Sees,
    Tables,
    Agent,
    Skill,
    Cancel,
    Save,
}

/// New profile, and Edit: what it uses, reads and sees; a new one also its agent.
struct ProfileForm {
    editing: Option<usize>,
    name: TextInput,
    conns: Vec<bool>,
    conn_cursor: usize,
    sql: bool,
    sees: Sees,
    /// Checked tables, by their selector.
    checked: Vec<String>,
    table_cursor: usize,
    /// 0 none, else CLIENTS index + 1.
    agent: usize,
    open: Option<usize>,
    skill: bool,
    focus: PRow,
    error: Option<(PRow, String)>,
}

impl ProfileForm {
    fn new(taken: &[String], profiles: &[Profile]) -> Self {
        let name = std::iter::once("assistant".to_string())
            .chain((2..).map(|n| format!("assistant-{n}")))
            .find(|name| !taken.contains(name))
            .unwrap_or_default();
        Self {
            editing: None,
            name: TextInput::new(name),
            conns: CONNS.iter().map(|conn| conn.name == "pg-dev").collect(),
            conn_cursor: 0,
            sql: true,
            sees: Sees::All,
            checked: Vec::new(),
            table_cursor: 0,
            // The first agent here that no profile starts yet.
            agent: (0..CLIENTS.len())
                .find(|at| CLIENTS[*at].found && !profiles.iter().any(|p| p.agents.contains(at)))
                .map_or(0, |at| at + 1),
            open: None,
            skill: true,
            focus: PRow::Name,
            error: None,
        }
    }

    fn edit(index: usize, profile: &Profile) -> Self {
        Self {
            editing: Some(index),
            name: TextInput::new(profile.name.clone()),
            conns: (0..CONNS.len())
                .map(|at| profile.conns.iter().any(|(conn, _)| *conn == at))
                .collect(),
            conn_cursor: 0,
            sql: profile.sql,
            sees: profile.sees,
            checked: profile.tables.clone(),
            table_cursor: 0,
            agent: 0,
            open: None,
            skill: false,
            focus: PRow::Conns,
            error: None,
        }
    }

    fn rows(&self) -> Vec<PRow> {
        let mut rows = Vec::new();
        if self.editing.is_none() {
            rows.push(PRow::Name);
        }
        rows.extend([PRow::Conns, PRow::Reads, PRow::Sees]);
        if self.sees != Sees::All {
            rows.push(PRow::Tables);
        }
        if self.editing.is_none() {
            rows.push(PRow::Agent);
            if self.agent > 0 {
                rows.push(PRow::Skill);
            }
        }
        rows.extend([PRow::Cancel, PRow::Save]);
        rows
    }

    /// The checked connections' tables, a group each, each with whether it is checked.
    fn table_groups(&self) -> Vec<(String, Vec<(String, bool)>)> {
        CONNS
            .iter()
            .zip(&self.conns)
            .filter(|(_, on)| **on)
            .map(|(conn, _)| {
                (
                    conn.name.to_string(),
                    conn.tables
                        .iter()
                        .map(|table| {
                            // Under its connection, a table is named without its database.
                            let shown = table.split_once('.').map_or(*table, |(_, rest)| rest);
                            (shown.to_string(), self.checked.iter().any(|t| t == table))
                        })
                        .collect(),
                )
            })
            .collect()
    }

    fn table_at(&self, at: usize) -> Option<String> {
        CONNS
            .iter()
            .zip(&self.conns)
            .filter(|(_, on)| **on)
            .flat_map(|(conn, _)| conn.tables.iter())
            .nth(at)
            .map(|table| table.to_string())
    }

    fn picked_conns(&self) -> Vec<&'static str> {
        CONNS
            .iter()
            .zip(&self.conns)
            .filter(|(_, on)| **on)
            .map(|(conn, _)| conn.name)
            .collect()
    }

    fn problem(&self, taken: &[String]) -> Option<(PRow, String)> {
        let name = self.name.trim();
        if self.editing.is_none() {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
            {
                return Some((
                    PRow::Name,
                    "a name takes letters, digits, - and _: it goes into the agent's config".into(),
                ));
            }
            if taken.iter().any(|taken| taken == name) {
                return Some((PRow::Name, format!("{name} is a profile already")));
            }
        }
        if self.picked_conns().is_empty() {
            return Some((PRow::Conns, "check at least one connection".into()));
        }
        if self.sees != Sees::All && self.checked.is_empty() {
            return Some((PRow::Tables, "check at least one table".into()));
        }
        None
    }

    fn key(&mut self, key: KeyEvent, taken: &[String]) -> Outcome {
        if self.focus == PRow::Agent
            && (self.open.is_some()
                || !matches!(
                    key.code,
                    KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down | KeyCode::Esc
                ))
            && dropdown_key(&mut self.open, &mut self.agent, CLIENTS.len() + 1, key.code)
        {
            return Outcome::Stay;
        }
        match self.focus {
            PRow::Conns => {
                if grid_key(&[CONNS.len()], &mut self.conn_cursor, key.code) {
                    return Outcome::Stay;
                }
                if matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter) {
                    self.conns[self.conn_cursor] = !self.conns[self.conn_cursor];
                    self.error = None;
                    return Outcome::Stay;
                }
            }
            PRow::Tables => {
                let sizes: Vec<usize> = self
                    .table_groups()
                    .iter()
                    .map(|(_, items)| items.len())
                    .collect();
                if grid_key(&sizes, &mut self.table_cursor, key.code) {
                    return Outcome::Stay;
                }
                if matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter)
                    && let Some(table) = self.table_at(self.table_cursor)
                {
                    match self.checked.iter().position(|t| *t == table) {
                        Some(at) => {
                            self.checked.remove(at);
                        }
                        None => self.checked.push(table),
                    }
                    self.error = None;
                    return Outcome::Stay;
                }
            }
            PRow::Reads => match key.code {
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                    self.sql = !self.sql;
                    return Outcome::Stay;
                }
                _ => {}
            },
            PRow::Sees => {
                let order = [Sees::All, Sees::Except, Sees::Only];
                let at = order
                    .iter()
                    .position(|sees| *sees == self.sees)
                    .unwrap_or(0);
                match key.code {
                    KeyCode::Left => {
                        self.sees = order[at.saturating_sub(1)];
                        return Outcome::Stay;
                    }
                    KeyCode::Right | KeyCode::Char(' ') => {
                        self.sees = order[(at + 1) % 3];
                        return Outcome::Stay;
                    }
                    _ => {}
                }
            }
            PRow::Skill if matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter) => {
                self.skill = !self.skill;
                return Outcome::Stay;
            }
            PRow::Name
                if !matches!(
                    key.code,
                    KeyCode::Tab
                        | KeyCode::BackTab
                        | KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::Esc
                        | KeyCode::Enter
                ) =>
            {
                self.name.handle_key(key);
                self.error = None;
                return Outcome::Stay;
            }
            _ => {}
        }
        let rows = self.rows();
        let entering = !matches!(self.focus, PRow::Conns | PRow::Tables);
        match form_key(&rows, &mut self.focus, PRow::Cancel, PRow::Save, key) {
            Some(Outcome::Submit) => match self.problem(taken) {
                Some(problem) => {
                    self.focus = problem.0;
                    self.error = Some(problem);
                    Outcome::Stay
                }
                None => Outcome::Submit,
            },
            Some(outcome) => {
                // Into a grid from above or below: its first or its last row.
                if entering {
                    let from_below = key.code == KeyCode::Up || key.code == KeyCode::BackTab;
                    match self.focus {
                        PRow::Conns => {
                            self.conn_cursor = panel::grid_enter(
                                &panel::grid_rows(&[CONNS.len()]),
                                self.conn_cursor,
                                from_below,
                            )
                        }
                        PRow::Tables => {
                            let sizes: Vec<usize> = self
                                .table_groups()
                                .iter()
                                .map(|(_, items)| items.len())
                                .collect();
                            self.table_cursor = panel::grid_enter(
                                &panel::grid_rows(&sizes),
                                self.table_cursor,
                                from_below,
                            );
                        }
                        _ => {}
                    }
                }
                outcome
            }
            None if key.code == KeyCode::Enter => {
                self.focus = walk(&rows, self.focus, 1);
                Outcome::Stay
            }
            None => Outcome::Stay,
        }
    }

    fn agent_spans(model: &Model, agent: usize, profiles: &[Profile]) -> Vec<Span<'static>> {
        let say = |text: String| Span::styled(text, panel::fg(model, Role::Foreground));
        let muted = |text: String| Span::styled(text, panel::fg(model, Role::Muted));
        if agent == 0 {
            return vec![
                say("None".into()),
                muted("  connect one later with a".into()),
            ];
        }
        let client = &CLIENTS[agent - 1];
        vec![
            say(format!("{:<15}", client.name)),
            muted(client_state(agent - 1, profiles)),
        ]
    }

    fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        model: &Model,
        hits: &mut HitMap,
        profiles: &[Profile],
    ) {
        let mut lines = Lines::new(panel::value_width(area));
        let rows = self.rows();
        let at = |row: PRow| rows.iter().position(|r| *r == row).unwrap_or(0);
        let focused = |row: PRow| self.focus == row;
        let invalid = |row: PRow| self.error.as_ref().is_some_and(|(at, _)| *at == row);
        let muted = panel::fg(model, Role::Muted);
        if self.editing.is_none() {
            lines.section(model, "WHO");
            lines.text(
                model,
                at(PRow::Name),
                "Name",
                &self.name,
                "",
                "letters, digits, - and _",
                focused(PRow::Name),
                invalid(PRow::Name),
            );
        }
        lines.section(model, "WHAT IT READS");
        let items: Vec<(String, bool)> = CONNS
            .iter()
            .zip(&self.conns)
            .map(|(conn, on)| (conn.name.to_string(), *on))
            .collect();
        lines.grid(
            model,
            at(PRow::Conns),
            "Connections",
            &[(String::new(), items)],
            focused(PRow::Conns).then_some(self.conn_cursor),
        );
        if self.conns[3] {
            lines.aside(
                "pg-prod is production: agents read it, never write to it",
                muted,
            );
        }
        lines.segmented(
            model,
            at(PRow::Reads),
            "Reads",
            &["Tables", "Tables and SELECT"],
            usize::from(self.sql),
            focused(PRow::Reads),
            "",
        );
        lines.aside(
            if self.sql {
                "browse and describe, and one SELECT at a time, 1000 rows at most"
            } else {
                "browse, describe and read pages, no SQL"
            },
            muted,
        );
        lines.segmented(
            model,
            at(PRow::Sees),
            "Sees",
            &["Everything", "Everything, except…", "Only…"],
            match self.sees {
                Sees::All => 0,
                Sees::Except => 1,
                Sees::Only => 2,
            },
            focused(PRow::Sees),
            "",
        );
        match self.sees {
            Sees::All => lines.aside("every table of its connections", muted),
            Sees::Except => lines.aside("the tables checked below are hidden from it", muted),
            Sees::Only => lines.aside("it sees only the tables checked below", muted),
        }
        if self.sees != Sees::All {
            lines.grid(
                model,
                at(PRow::Tables),
                "Tables",
                &self.table_groups(),
                focused(PRow::Tables).then_some(self.table_cursor),
            );
        }
        if self.editing.is_none() {
            lines.section(model, "AGENT");
            let open = self.open.filter(|_| focused(PRow::Agent)).map(|highlight| {
                (
                    (0..=CLIENTS.len())
                        .map(|agent| Self::agent_spans(model, agent, profiles))
                        .collect(),
                    highlight,
                )
            });
            lines.dropdown(
                model,
                at(PRow::Agent),
                "Connect",
                Self::agent_spans(model, self.agent, profiles),
                focused(PRow::Agent),
                false,
                open,
            );
            if self.agent > 0 {
                lines.aside(format!("writes {}", CLIENTS[self.agent - 1].file), muted);
                lines.toggle(
                    model,
                    at(PRow::Skill),
                    "Skill file",
                    self.skill,
                    if self.skill {
                        "tells the agent how Dexo's tools behave"
                    } else {
                        "no skill file"
                    },
                    focused(PRow::Skill),
                );
            }
        }
        lines.error(
            model,
            self.error.as_ref().map(|(_, message)| message.as_str()),
        );
        lines.section(model, "RESULT");
        let name = if self.editing.is_some() {
            self.name.as_str().to_string()
        } else {
            self.name.trim().to_string()
        };
        let conns = self.picked_conns();
        if conns.is_empty() {
            lines.note("check the connections it may use", muted);
        } else {
            lines.note(
                format!(
                    "{name} reads {}: {}, {}.",
                    conns.join(", "),
                    if self.sql {
                        "tables and SELECT"
                    } else {
                        "tables only"
                    },
                    sees_words(self.sees, &self.checked)
                ),
                panel::fg(model, Role::Foreground),
            );
        }
        if self.editing.is_none() {
            if self.agent > 0 {
                let client = &CLIENTS[self.agent - 1];
                lines.note(
                    format!("{} starts it, enabled, from {}.", client.name, client.file),
                    panel::fg(model, Role::Foreground),
                );
                if let Some(other) = profiles
                    .iter()
                    .find(|p| p.agents.contains(&(self.agent - 1)))
                {
                    lines.note(
                        format!("{} used {} until now.", client.name, other.name),
                        panel::fg(model, Role::Warning),
                    );
                }
            }
            lines.note(
                "It cannot write: w on a connection lets it, asking you each time.",
                muted,
            );
        }
        if matches!(self.focus, PRow::Cancel | PRow::Save) {
            lines.focus_end();
        }
        let label = if self.editing.is_some() {
            "Save"
        } else {
            "Create profile"
        };
        let title = match self.editing {
            Some(_) => format!("Edit {name}"),
            None => "New profile".to_string(),
        };
        panel::draw(
            frame,
            area,
            model,
            hits,
            &title,
            lines,
            &buttons(focused(PRow::Cancel), focused(PRow::Save), label),
            match (self.focus, self.open) {
                (_, Some(_)) => &[("↑/↓", "choose"), ("Enter", "pick"), ("Esc", "close")],
                (PRow::Conns | PRow::Tables, _) => &[
                    ("↑/↓/←/→", "move"),
                    ("Space", "check"),
                    ("Tab", "next"),
                    ("Esc", "cancel"),
                ],
                (PRow::Reads | PRow::Sees, _) => {
                    &[("←/→", "choose"), ("Tab", "next"), ("Esc", "cancel")]
                }
                (PRow::Agent, _) => &[
                    ("Enter", "open"),
                    ("←/→", "change"),
                    ("Tab", "next"),
                    ("Esc", "cancel"),
                ],
                (PRow::Skill, _) => &[("Space", "toggle"), ("Tab", "next"), ("Esc", "cancel")],
                (PRow::Name, _) => &[("Tab", "next"), ("Esc", "cancel")],
                (PRow::Save, _) => &[("Enter", "save"), ("←/→", "switch"), ("Esc", "cancel")],
                (PRow::Cancel, _) => &[("Enter", "cancel"), ("←/→", "switch")],
            },
        );
    }
}

/// What an agent's file says now: set up and with which profile, or found here or not.
fn client_state(client: usize, profiles: &[Profile]) -> String {
    match profiles
        .iter()
        .find(|profile| profile.agents.contains(&client))
    {
        Some(profile) => format!("set up · {}", profile.name),
        None if CLIENTS[client].found => "found".into(),
        None => "not found here".into(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WRow {
    Conn,
    Kinds,
    Lasts,
    Cancel,
    Create,
}

/// Writes: what the profile may change on one connection, and for how long. Every write
/// asks.
struct WritesForm {
    profile: usize,
    /// The connections it can be for: indexes into the profile's.
    offered: Vec<usize>,
    conn: usize,
    open: Option<usize>,
    kinds: [bool; 3],
    cursor: usize,
    lasts: usize,
    focus: WRow,
    error: Option<String>,
}

impl WritesForm {
    fn new(index: usize, profile: &Profile, on: Option<usize>) -> Option<Self> {
        let offered: Vec<usize> = profile
            .conns
            .iter()
            .enumerate()
            .filter(|(_, (conn, _))| CONNS[*conn].writable())
            .map(|(at, _)| at)
            .collect();
        if offered.is_empty() {
            return None;
        }
        let conn = on
            .and_then(|on| offered.iter().position(|at| *at == on))
            .unwrap_or(0);
        let mut form = Self {
            profile: index,
            offered,
            conn,
            open: None,
            kinds: [true, false, false],
            cursor: 0,
            lasts: 1,
            focus: WRow::Kinds,
            error: None,
        };
        form.load(profile);
        Some(form)
    }

    /// What the connection has now, to change it.
    fn load(&mut self, profile: &Profile) {
        if let Some((_, Some(access))) = profile.conns.get(self.offered[self.conn]) {
            self.kinds = access.kinds;
            self.lasts = access.lasts;
        }
    }

    fn rows(&self) -> Vec<WRow> {
        vec![
            WRow::Conn,
            WRow::Kinds,
            WRow::Lasts,
            WRow::Cancel,
            WRow::Create,
        ]
    }

    fn key(&mut self, key: KeyEvent, profile: &Profile) -> Outcome {
        if self.focus == WRow::Conn
            && (self.open.is_some()
                || !matches!(
                    key.code,
                    KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down | KeyCode::Esc
                ))
        {
            let before = self.conn;
            if dropdown_key(&mut self.open, &mut self.conn, self.offered.len(), key.code) {
                if self.conn != before {
                    self.load(profile);
                }
                return Outcome::Stay;
            }
        }
        match self.focus {
            WRow::Kinds => {
                if grid_key(&[3], &mut self.cursor, key.code) {
                    return Outcome::Stay;
                }
                if matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter) {
                    self.kinds[self.cursor] = !self.kinds[self.cursor];
                    self.error = None;
                    return Outcome::Stay;
                }
            }
            WRow::Lasts => match key.code {
                KeyCode::Left => {
                    self.lasts = self.lasts.saturating_sub(1);
                    return Outcome::Stay;
                }
                KeyCode::Right | KeyCode::Char(' ') => {
                    self.lasts = (self.lasts + 1) % LASTS.len();
                    return Outcome::Stay;
                }
                _ => {}
            },
            _ => {}
        }
        let rows = self.rows();
        let entering = self.focus != WRow::Kinds;
        match form_key(&rows, &mut self.focus, WRow::Cancel, WRow::Create, key) {
            Some(Outcome::Submit) if !self.kinds.iter().any(|on| *on) => {
                self.focus = WRow::Kinds;
                self.error = Some("check what it may change".into());
                Outcome::Stay
            }
            Some(outcome) => {
                if entering && self.focus == WRow::Kinds {
                    let from_below = key.code == KeyCode::Up || key.code == KeyCode::BackTab;
                    self.cursor =
                        panel::grid_enter(&panel::grid_rows(&[3]), self.cursor, from_below);
                }
                outcome
            }
            None if key.code == KeyCode::Enter => {
                self.focus = walk(&rows, self.focus, 1);
                Outcome::Stay
            }
            None => Outcome::Stay,
        }
    }

    fn conn_spans(model: &Model, profile: &Profile, at: usize) -> Vec<Span<'static>> {
        let (conn, access) = &profile.conns[at];
        let conn = &CONNS[*conn];
        let mut spans = vec![
            Span::styled(
                format!("{:<12}", conn.name),
                panel::fg(model, Role::Foreground),
            ),
            Span::styled(
                format!("{:<12}", conn.env),
                panel::fg(
                    model,
                    if conn.env == "staging" {
                        Role::Staging
                    } else {
                        Role::Development
                    },
                ),
            ),
        ];
        if access.is_some() {
            spans.push(Span::styled("✎ has writes", panel::fg(model, Role::Muted)));
        }
        spans
    }

    fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        model: &Model,
        hits: &mut HitMap,
        profile: &Profile,
    ) {
        let mut lines = Lines::new(panel::value_width(area));
        let at = |row: WRow| self.rows().iter().position(|r| *r == row).unwrap_or(0);
        let focused = |row: WRow| self.focus == row;
        let muted = panel::fg(model, Role::Muted);
        let current = &profile.conns[self.offered[self.conn]];
        let conn = &CONNS[current.0];
        lines.section(model, "WHERE");
        let open = self.open.filter(|_| focused(WRow::Conn)).map(|highlight| {
            (
                self.offered
                    .iter()
                    .map(|at| Self::conn_spans(model, profile, *at))
                    .collect(),
                highlight,
            )
        });
        lines.dropdown(
            model,
            at(WRow::Conn),
            "Connection",
            Self::conn_spans(model, profile, self.offered[self.conn]),
            focused(WRow::Conn),
            false,
            open,
        );
        for (other, _) in &profile.conns {
            if !CONNS[*other].writable() {
                lines.aside(
                    format!(
                        "{} is not offered: production never takes agents' writes",
                        CONNS[*other].name
                    ),
                    muted,
                );
            }
        }
        lines.aside(
            format!("covers everything {} sees there", profile.name),
            muted,
        );
        lines.section(model, "WHAT");
        let items: Vec<(String, bool)> = KINDS
            .iter()
            .zip(&self.kinds)
            .map(|((label, _), on)| (label.to_string(), *on))
            .collect();
        lines.grid(
            model,
            at(WRow::Kinds),
            "May",
            &[(String::new(), items)],
            focused(WRow::Kinds).then_some(self.cursor),
        );
        let (label, covers) = KINDS[self.cursor];
        lines.aside(format!("{label}: {covers}"), muted);
        lines.section(model, "HOW LONG");
        let options: Vec<&str> = LASTS.iter().map(|(short, _)| *short).collect();
        lines.segmented(
            model,
            at(WRow::Lasts),
            "For",
            &options,
            self.lasts,
            focused(WRow::Lasts),
            "",
        );
        lines.error(model, self.error.as_deref());
        lines.section(model, "RESULT");
        let changes: Vec<&str> = [(0, "rows"), (1, "the schema")]
            .into_iter()
            .filter(|(at, _)| self.kinds[*at])
            .map(|(_, word)| word)
            .collect();
        let mut kinds = Vec::new();
        if !changes.is_empty() {
            kinds.push(format!("change {}", changes.join(" and ")));
        }
        if self.kinds[2] {
            kinds.push("end sessions".to_string());
        }
        if kinds.is_empty() {
            lines.note("check what it may change", muted);
        } else {
            lines.note(
                format!(
                    "{} may {} in {}, asking you before each write, {}.",
                    profile.name,
                    kinds.join(" and "),
                    conn.name,
                    LASTS[self.lasts].1
                ),
                panel::fg(model, Role::Foreground),
            );
        }
        lines.note(
            "Each write waits up to 2 min on Approvals, then is refused.",
            muted,
        );
        if let Some(access) = &current.1 {
            lines.note(
                format!(
                    "Replaces what it has there now: {}, {}.",
                    kinds_words(&access.kinds),
                    access.left
                ),
                panel::fg(model, Role::Warning),
            );
        }
        if self.lasts == LASTS.len() - 1 {
            lines.note(
                "! no time limit: r on the connection takes it back",
                panel::bold(model, Role::Warning),
            );
        }
        if matches!(self.focus, WRow::Cancel | WRow::Create) {
            lines.focus_end();
        }
        let label = if current.1.is_some() {
            "Change writes"
        } else {
            "Allow writes"
        };
        panel::draw(
            frame,
            area,
            model,
            hits,
            &format!("Writes of {}", profile.name),
            lines,
            &buttons(focused(WRow::Cancel), focused(WRow::Create), label),
            match (self.focus, self.open) {
                (_, Some(_)) => &[("↑/↓", "choose"), ("Enter", "pick"), ("Esc", "close")],
                (WRow::Conn, _) => &[
                    ("Enter", "open"),
                    ("←/→", "change"),
                    ("Tab", "next"),
                    ("Esc", "cancel"),
                ],
                (WRow::Kinds, _) => &[
                    ("↑/↓/←/→", "move"),
                    ("Space", "check"),
                    ("Tab", "next"),
                    ("Esc", "cancel"),
                ],
                (WRow::Lasts, _) => &[("←/→", "choose"), ("Tab", "next"), ("Esc", "cancel")],
                (WRow::Create, _) => &[("Enter", "allow"), ("←/→", "switch"), ("Esc", "cancel")],
                (WRow::Cancel, _) => &[("Enter", "cancel"), ("←/→", "switch")],
            },
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ARow {
    Agent,
    Skill,
    Cancel,
    Connect,
}

/// Connect an agent: its config written to start Dexo with this profile.
struct AgentForm {
    profile: usize,
    agent: usize,
    open: Option<usize>,
    skill: bool,
    focus: ARow,
}

impl AgentForm {
    fn rows(&self) -> Vec<ARow> {
        vec![ARow::Agent, ARow::Skill, ARow::Cancel, ARow::Connect]
    }

    fn key(&mut self, key: KeyEvent) -> Outcome {
        if self.focus == ARow::Agent
            && (self.open.is_some()
                || !matches!(
                    key.code,
                    KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down | KeyCode::Esc
                ))
            && dropdown_key(&mut self.open, &mut self.agent, CLIENTS.len(), key.code)
        {
            return Outcome::Stay;
        }
        if self.focus == ARow::Skill && matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter) {
            self.skill = !self.skill;
            return Outcome::Stay;
        }
        let rows = self.rows();
        form_key(&rows, &mut self.focus, ARow::Cancel, ARow::Connect, key).unwrap_or(Outcome::Stay)
    }

    fn spans(model: &Model, agent: usize, profiles: &[Profile]) -> Vec<Span<'static>> {
        vec![
            Span::styled(
                format!("{:<15}", CLIENTS[agent].name),
                panel::fg(model, Role::Foreground),
            ),
            Span::styled(client_state(agent, profiles), panel::fg(model, Role::Muted)),
        ]
    }

    fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        model: &Model,
        hits: &mut HitMap,
        profiles: &[Profile],
    ) {
        let profile = &profiles[self.profile];
        let client = &CLIENTS[self.agent];
        let mut lines = Lines::new(panel::value_width(area));
        let focused = |row: ARow| self.focus == row;
        let muted = panel::fg(model, Role::Muted);
        lines.section(model, "AGENT");
        let open = self.open.filter(|_| focused(ARow::Agent)).map(|highlight| {
            (
                (0..CLIENTS.len())
                    .map(|agent| Self::spans(model, agent, profiles))
                    .collect(),
                highlight,
            )
        });
        lines.dropdown(
            model,
            0,
            "Agent",
            Self::spans(model, self.agent, profiles),
            focused(ARow::Agent),
            false,
            open,
        );
        lines.aside(
            format!("writes {}, the old one kept as a backup", client.file),
            muted,
        );
        if !client.found {
            lines.aside(
                "not found on this machine: the file is written for when it is",
                muted,
            );
        }
        lines.toggle(
            model,
            1,
            "Skill file",
            self.skill,
            if self.skill {
                "tells the agent how Dexo's tools behave"
            } else {
                "no skill file"
            },
            focused(ARow::Skill),
        );
        lines.blank();
        lines.section(model, "RESULT");
        lines.note(
            format!("{} starts Dexo with {}.", client.name, profile.name),
            panel::fg(model, Role::Foreground),
        );
        if let Some(other) = profiles
            .iter()
            .find(|other| other.name != profile.name && other.agents.contains(&self.agent))
        {
            lines.note(
                format!("It used {} until now.", other.name),
                panel::fg(model, Role::Warning),
            );
        }
        lines.note(
            format!("→ restart {} once it is written", client.name),
            muted,
        );
        if matches!(self.focus, ARow::Cancel | ARow::Connect) {
            lines.focus_end();
        }
        panel::draw(
            frame,
            area,
            model,
            hits,
            &format!("Connect an agent to {}", profile.name),
            lines,
            &buttons(focused(ARow::Cancel), focused(ARow::Connect), "Connect"),
            match (self.focus, self.open) {
                (_, Some(_)) => &[("↑/↓", "choose"), ("Enter", "pick"), ("Esc", "close")],
                (ARow::Agent, _) => &[
                    ("Enter", "open"),
                    ("←/→", "change"),
                    ("y", "copy command"),
                    ("Esc", "cancel"),
                ],
                (ARow::Skill, _) => &[("Space", "toggle"), ("Tab", "next"), ("Esc", "cancel")],
                (ARow::Connect, _) => &[("Enter", "connect"), ("←/→", "switch"), ("Esc", "cancel")],
                (ARow::Cancel, _) => &[("Enter", "cancel"), ("←/→", "switch")],
            },
        );
    }
}

enum Form {
    Profile(ProfileForm),
    Writes(WritesForm),
    Agent(AgentForm),
}

// -- The three looks ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Sheet,
    Tabs,
    Columns,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Conn(usize),
    Agent(usize),
}

struct App {
    look: Look,
    model: Model,
    profiles: Vec<Profile>,
    picked: usize,
    /// The keys are in the detail, not the list.
    detail: bool,
    row: usize,
    /// Tabs: 0 Connections, 1 Agents.
    tab: usize,
    form: Option<Form>,
    notice: Option<(String, Role, Instant)>,
}

impl App {
    fn new() -> Self {
        let mut model = Model::default();
        update(&mut model, Action::GoToScreen(Screen::Agents));
        model.theme = theme_for(Mode::Dark, DEFAULT_ACCENT);
        model.capabilities = TerminalCapabilities::detect();
        Self {
            look: Look::Sheet,
            model,
            profiles: profiles(),
            picked: 0,
            detail: false,
            row: 0,
            tab: 0,
            form: None,
            notice: None,
        }
    }

    fn profile(&self) -> &Profile {
        &self.profiles[self.picked]
    }

    fn names(&self) -> Vec<String> {
        self.profiles
            .iter()
            .map(|profile| profile.name.clone())
            .collect()
    }

    fn rows(&self) -> Vec<Row> {
        let profile = self.profile();
        let conns = (0..profile.conns.len()).map(Row::Conn);
        let agents = (0..profile.agents.len()).map(Row::Agent);
        match (self.look, self.tab) {
            (Look::Tabs, 0) => conns.collect(),
            (Look::Tabs, _) => agents.collect(),
            _ => conns.chain(agents).collect(),
        }
    }

    fn current(&self) -> Option<Row> {
        self.rows().get(self.row).copied()
    }

    fn say(&mut self, text: impl Into<String>, role: Role) {
        self.notice = Some((text.into(), role, Instant::now()));
    }

    fn open_writes(&mut self) {
        let on = match (self.detail, self.current()) {
            (true, Some(Row::Conn(at))) => Some(at),
            _ => None,
        };
        match WritesForm::new(self.picked, self.profile(), on) {
            Some(form) => self.form = Some(Form::Writes(form)),
            None => self.say(
                format!(
                    "{} has no connection that takes writes: production never does",
                    self.profile().name
                ),
                Role::Warning,
            ),
        }
    }

    fn key(&mut self, key: KeyEvent) {
        if let Some(form) = self.form.as_mut() {
            let names = self
                .profiles
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>();
            let outcome = match form {
                Form::Profile(form) => form.key(key, &names),
                Form::Writes(form) => form.key(key, &self.profiles[form.profile]),
                Form::Agent(form) => {
                    if key.code == KeyCode::Char('y') {
                        let name = CLIENTS[form.agent].name;
                        self.say(
                            format!("Copied {name}'s command to start Dexo"),
                            Role::Success,
                        );
                        return;
                    }
                    form.key(key)
                }
            };
            match outcome {
                Outcome::Stay => {}
                Outcome::Cancel => {
                    self.form = None;
                    self.say("Cancelled: nothing changed", Role::Muted);
                }
                Outcome::Submit => self.submit(),
            }
            return;
        }
        let rows = self.rows().len();
        match key.code {
            KeyCode::Tab => self.detail = !self.detail,
            KeyCode::Right if !self.detail => self.detail = true,
            KeyCode::Left | KeyCode::Esc
                if self.detail && !(self.look == Look::Tabs && key.code == KeyCode::Left) =>
            {
                self.detail = false
            }
            KeyCode::Left | KeyCode::Right if self.detail && self.look == Look::Tabs => {
                self.tab = 1 - self.tab;
                self.row = 0;
            }
            KeyCode::Up if self.detail => self.row = self.row.saturating_sub(1),
            KeyCode::Down if self.detail => self.row = (self.row + 1).min(rows.saturating_sub(1)),
            KeyCode::Up => {
                self.picked = self.picked.saturating_sub(1);
                self.row = 0;
            }
            KeyCode::Down => {
                self.picked = (self.picked + 1).min(self.profiles.len() - 1);
                self.row = 0;
            }
            KeyCode::Char('n') => {
                self.form = Some(Form::Profile(ProfileForm::new(
                    &self.names(),
                    &self.profiles,
                )))
            }
            KeyCode::Enter => {
                self.form = Some(Form::Profile(ProfileForm::edit(
                    self.picked,
                    self.profile(),
                )))
            }
            KeyCode::Char('w') => self.open_writes(),
            KeyCode::Char('a') => {
                let agent = match self.current() {
                    Some(Row::Agent(at)) if self.detail => self.profile().agents[at],
                    // One here that this profile does not start yet, else any it does not.
                    _ => {
                        let free = |agent: &usize| !self.profile().agents.contains(agent);
                        (0..CLIENTS.len())
                            .find(|agent| free(agent) && CLIENTS[*agent].found)
                            .or_else(|| (0..CLIENTS.len()).find(free))
                            .unwrap_or(0)
                    }
                };
                self.form = Some(Form::Agent(AgentForm {
                    profile: self.picked,
                    agent,
                    open: None,
                    skill: true,
                    focus: ARow::Agent,
                }));
            }
            KeyCode::Char('r') => match (self.detail, self.current()) {
                (true, Some(Row::Conn(at))) => {
                    let picked = self.picked;
                    let (conn, access) = &mut self.profiles[picked].conns[at];
                    let name = CONNS[*conn].name;
                    if access.take().is_some() {
                        self.say(
                            format!("Revoked the writes on {name}: a write waiting for approval there is refused"),
                            Role::Success,
                        );
                    } else {
                        self.say(format!("{name} is read only already"), Role::Muted);
                    }
                }
                _ => self.say("Pick the connection first: Tab, then r", Role::Muted),
            },
            KeyCode::Char('R') => {
                let mut count = 0;
                for profile in &mut self.profiles {
                    for (_, access) in &mut profile.conns {
                        count += usize::from(access.take().is_some());
                    }
                }
                self.say(
                    format!("Dexo would ask first, then stop every write: {count} taken back"),
                    Role::Warning,
                );
            }
            KeyCode::Char('e') => {
                let picked = self.picked;
                let profile = &mut self.profiles[picked];
                profile.enabled = !profile.enabled;
                let text = if profile.enabled {
                    format!("Enabled {} (Dexo asks first)", profile.name)
                } else {
                    format!("Disabled {}: agents can no longer connect", profile.name)
                };
                self.say(text, Role::Success);
            }
            KeyCode::Char('x') => {
                let name = self.profile().name.clone();
                self.say(
                    format!("Dexo would ask: delete {name}? Its writes go with it, and its agents stop working"),
                    Role::Warning,
                );
            }
            KeyCode::Char('y') => match self.current() {
                Some(Row::Agent(at)) if self.detail => {
                    let name = CLIENTS[self.profile().agents[at]].name;
                    self.say(
                        format!("Copied {name}'s command to start Dexo"),
                        Role::Success,
                    );
                }
                _ => self.say("Pick the agent first: Tab, then y", Role::Muted),
            },
            _ => {}
        }
    }

    fn submit(&mut self) {
        match self.form.take() {
            Some(Form::Profile(form)) => {
                let tables = if form.sees == Sees::All {
                    Vec::new()
                } else {
                    form.checked.clone()
                };
                let conns: Vec<usize> = (0..CONNS.len()).filter(|at| form.conns[*at]).collect();
                match form.editing {
                    Some(index) => {
                        let profile = &mut self.profiles[index];
                        let kept = profile.conns.clone();
                        profile.conns = conns
                            .into_iter()
                            .map(|conn| {
                                let access = kept
                                    .iter()
                                    .find(|(at, _)| *at == conn)
                                    .and_then(|(_, a)| a.clone());
                                (conn, access)
                            })
                            .collect();
                        profile.sql = form.sql;
                        profile.sees = form.sees;
                        profile.tables = tables;
                        let name = profile.name.clone();
                        self.say(format!("Saved {name}"), Role::Success);
                    }
                    None => {
                        let agents: Vec<usize> = (form.agent > 0)
                            .then(|| form.agent - 1)
                            .into_iter()
                            .collect();
                        for profile in &mut self.profiles {
                            profile.agents.retain(|agent| !agents.contains(agent));
                        }
                        let name = form.name.trim().to_string();
                        self.profiles.push(Profile {
                            name: name.clone(),
                            enabled: true,
                            conns: conns.into_iter().map(|conn| (conn, None)).collect(),
                            sql: form.sql,
                            sees: form.sees,
                            tables,
                            agents: agents.clone(),
                        });
                        self.picked = self.profiles.len() - 1;
                        self.row = 0;
                        let with = agents
                            .first()
                            .map(|agent| format!(", and {} starts it", CLIENTS[*agent].name))
                            .unwrap_or_default();
                        self.say(format!("✓ Made {name}, enabled{with}"), Role::Success);
                    }
                }
            }
            Some(Form::Writes(form)) => {
                let profile = &mut self.profiles[form.profile];
                let at = form.offered[form.conn];
                profile.conns[at].1 = Some(Access {
                    kinds: form.kinds,
                    lasts: form.lasts,
                    left: left_after(form.lasts),
                });
                let text = format!(
                    "✓ {} may write {} in {}, asking each time, {}",
                    profile.name,
                    kinds_words(&form.kinds),
                    CONNS[profile.conns[at].0].name,
                    LASTS[form.lasts].1
                );
                self.say(text, Role::Success);
            }
            Some(Form::Agent(form)) => {
                for profile in &mut self.profiles {
                    profile.agents.retain(|agent| *agent != form.agent);
                }
                let profile = &mut self.profiles[form.profile];
                profile.agents.push(form.agent);
                let text = format!(
                    "✓ {} written: {} starts Dexo with {}",
                    CLIENTS[form.agent].file, CLIENTS[form.agent].name, profile.name
                );
                self.say(text, Role::Success);
            }
            None => {}
        }
    }
}

fn style(model: &Model, role: Role) -> Style {
    model.theme.style(role, model.capabilities)
}

fn list_buttons() -> Vec<Button> {
    vec![
        Button::new(KeyCode::Char('n'), "New"),
        Button::new(KeyCode::Char('R'), "Stop all writes"),
    ]
}

fn profile_buttons(profile: &Profile) -> Vec<Button> {
    vec![
        Button::new(
            KeyCode::Char('e'),
            if profile.enabled { "Disable" } else { "Enable" },
        ),
        Button::new(KeyCode::Enter, "Edit"),
        Button::new(KeyCode::Char('x'), "Delete"),
    ]
}

fn draw_list(frame: &mut Frame, area: Rect, app: &App, hits: &mut HitMap) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let model = &app.model;
    let block = dexo_tui::render::pane_block(
        model,
        &format!("Profiles ({})", app.profiles.len()),
        !app.detail && app.form.is_none(),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let inner = widgets::list_actions(frame, inner, model, hits, &list_buttons());
    let width = app
        .profiles
        .iter()
        .map(|p| p.name.chars().count())
        .max()
        .unwrap_or(0);
    let compact = inner.width < 40;
    let entries: Vec<Entry> = app
        .profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let writes = profile
                .conns
                .iter()
                .filter(|(_, access)| access.is_some())
                .count();
            Entry {
                spans: vec![
                    if profile.enabled {
                        Span::styled("●", style(model, Role::Success))
                    } else {
                        Span::styled("○", style(model, Role::Muted))
                    },
                    Span::raw(format!(" {:<width$}  ", profile.name)),
                    if compact {
                        Span::raw("")
                    } else {
                        Span::styled(
                            format!("{:<12}", agents_words(profile)),
                            style(model, Role::Muted),
                        )
                    },
                    if writes > 0 && compact {
                        Span::raw("✎")
                    } else if compact {
                        Span::raw("")
                    } else if writes > 0 {
                        Span::raw(" ✎ writes")
                    } else if profile.enabled {
                        Span::styled(" read only", style(model, Role::Muted))
                    } else {
                        Span::styled(" disabled", style(model, Role::Muted))
                    },
                ],
                target: Some(HitTarget::ListRow(index)),
                picked: index == app.picked,
                heading: false,
            }
        })
        .collect();
    widgets::entries(frame, inner, model, hits, &entries);
}

/// A section's title in the detail, then the keys that act on its rows.
fn heading(model: &Model, title: &str, keys: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {title}"),
            style(model, Role::Border).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("   {keys}"), style(model, Role::Muted)),
    ])
}

/// A row of the detail the keys walk: marked and reversed while it has them.
fn walked(spans: Vec<Span<'static>>, picked: bool, width: usize) -> Line<'static> {
    let mut all = vec![Span::raw(if picked { " > " } else { "   " })];
    all.extend(spans);
    let mut line = Line::from(all);
    if picked {
        let used = line.width();
        line.spans
            .push(Span::raw(" ".repeat(width.saturating_sub(used))));
        line = line.patch_style(Style::default().add_modifier(Modifier::REVERSED));
    }
    line
}

fn field(model: &Model, label: &str, value: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!(" {label:<12}  "),
        style(model, Role::Muted),
    )];
    spans.extend(value);
    Line::from(spans)
}

fn status_spans(model: &Model, profile: &Profile) -> Vec<Span<'static>> {
    if profile.enabled {
        vec![
            Span::styled("● ", style(model, Role::Success)),
            Span::raw("enabled: agents can connect"),
        ]
    } else {
        vec![
            Span::styled("○ ", style(model, Role::Muted)),
            Span::raw("disabled: agents cannot connect"),
        ]
    }
}

fn conn_line(
    model: &Model,
    profile: &Profile,
    at: usize,
    picked: bool,
    width: usize,
    short: bool,
) -> Line<'static> {
    let (conn, access) = &profile.conns[at];
    let conn = &CONNS[*conn];
    let (writes, role) = writes_words(conn, access, short);
    let env_role = match conn.env {
        "production" => Role::Production,
        "staging" => Role::Staging,
        _ => Role::Development,
    };
    let mut spans = vec![Span::raw(format!("{:<12}", conn.name))];
    if !short {
        spans.push(Span::styled(
            format!("{:<13}", conn.env),
            style(model, env_role),
        ));
    }
    spans.push(Span::styled(writes, style(model, role)));
    walked(spans, picked, width)
}

fn agent_line(
    model: &Model,
    profile: &Profile,
    at: usize,
    picked: bool,
    width: usize,
    short: bool,
) -> Line<'static> {
    let client = &CLIENTS[profile.agents[at]];
    let mut spans = vec![
        Span::styled("✓ ", style(model, Role::Success)),
        Span::raw(format!("{:<15}", client.name)),
    ];
    if !short {
        spans.push(Span::styled(
            client.file.to_string(),
            style(model, Role::Muted),
        ));
    }
    walked(spans, picked, width)
}

/// The detail pane's frame and buttons; what is under them.
fn detail_frame(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    hits: &mut HitMap,
    title: &str,
    buttons: &[Button],
) -> Rect {
    let model = &app.model;
    let block = dexo_tui::render::pane_block(model, title, app.detail);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let used = widgets::action_bar(frame, inner, model, hits, buttons, None);
    let gap = u16::from(inner.height > used + 3);
    Rect::new(
        inner.x,
        inner.y + used + gap,
        inner.width,
        inner.height.saturating_sub(used + gap),
    )
}

fn sheet(frame: &mut Frame, area: Rect, app: &App, hits: &mut HitMap) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let model = &app.model;
    let profile = app.profile();
    let body = detail_frame(
        frame,
        area,
        app,
        hits,
        &profile.name,
        &profile_buttons(profile),
    );
    let width = usize::from(body.width);
    let picked = |row: Row| app.detail && app.current() == Some(row);
    let mut lines = vec![field(model, "Status", status_spans(model, profile))];
    lines.extend(widgets::field_lines_with(
        model,
        &[
            widgets::FieldRow::Field("Reads", reads_words(profile).into()),
            widgets::FieldRow::Field("Sees", sees_words(profile.sees, &profile.tables)),
        ],
        body.width,
        12,
    ));
    lines.extend([
        Line::default(),
        heading(
            model,
            "CONNECTIONS",
            "each write asks you first · w writes · r revoke",
        ),
    ]);
    for at in 0..profile.conns.len() {
        lines.push(conn_line(
            model,
            profile,
            at,
            picked(Row::Conn(at)),
            width,
            false,
        ));
    }
    lines.push(Line::default());
    lines.push(heading(model, "AGENTS", "a connect · y copy its command"));
    if profile.agents.is_empty() {
        lines.push(Line::styled(
            "   none: a connects Claude Code, Codex, Cursor and the rest",
            style(model, Role::Muted),
        ));
    }
    for at in 0..profile.agents.len() {
        lines.push(agent_line(
            model,
            profile,
            at,
            picked(Row::Agent(at)),
            width,
            false,
        ));
    }
    // Scrolled to keep the walked row in sight on a short pane.
    let focus = lines
        .iter()
        .position(|line| line.spans.first().is_some_and(|span| span.content == " > "))
        .unwrap_or(0);
    let offset = (focus + 1).saturating_sub(usize::from(body.height));
    frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), body);
}

fn tabs(frame: &mut Frame, area: Rect, app: &App, hits: &mut HitMap) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let model = &app.model;
    let profile = app.profile();
    let body = detail_frame(
        frame,
        area,
        app,
        hits,
        &profile.name,
        &profile_buttons(profile),
    );
    let width = usize::from(body.width);
    let mut lines = vec![Line::from(
        [vec![Span::raw(" ")], status_spans(model, profile)].concat(),
    )];
    let reads = format!(
        "{} · sees {}",
        if profile.sql {
            "tables and SELECT"
        } else {
            "tables only"
        },
        sees_words(profile.sees, &profile.tables)
    );
    for part in dexo_tui::model::wrap_words(&reads, width.saturating_sub(3)) {
        lines.push(Line::styled(
            format!("   {part}"),
            style(model, Role::Muted),
        ));
    }
    lines.push(Line::default());
    let tab = |index: usize, label: String| {
        if app.tab == index {
            Span::styled(
                format!("[{label}]"),
                style(model, Role::Focus).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!(" {label} "), style(model, Role::Muted))
        }
    };
    lines.push(Line::from(vec![
        Span::raw(" "),
        tab(0, format!("Connections {}", profile.conns.len())),
        Span::raw("  "),
        tab(1, format!("Agents {}", profile.agents.len())),
    ]));
    let header =
        |text: String| Line::styled(text, style(model, Role::Muted).add_modifier(Modifier::BOLD));
    let picked = |row: Row| app.detail && app.current() == Some(row);
    if app.tab == 0 {
        lines.push(header(format!(
            "   {:<12}{:<13}{}",
            "CONNECTION", "ENVIRONMENT", "WRITES"
        )));
        for at in 0..profile.conns.len() {
            lines.push(conn_line(
                model,
                profile,
                at,
                picked(Row::Conn(at)),
                width,
                false,
            ));
        }
        lines.push(Line::default());
        lines.push(Line::styled(
            " w writes · r revoke · ←/→ Agents",
            style(model, Role::Muted),
        ));
    } else {
        lines.push(header(format!("   {:<17}{}", "AGENT", "ITS FILE")));
        if profile.agents.is_empty() {
            lines.push(Line::styled("   none yet", style(model, Role::Muted)));
        }
        for at in 0..profile.agents.len() {
            lines.push(agent_line(
                model,
                profile,
                at,
                picked(Row::Agent(at)),
                width,
                false,
            ));
        }
        lines.push(Line::default());
        lines.push(Line::styled(
            " a connect · y copy its command · ←/→ Connections",
            style(model, Role::Muted),
        ));
    }
    frame.render_widget(Paragraph::new(lines), body);
}

fn columns(frame: &mut Frame, middle: Rect, right: Rect, app: &App, hits: &mut HitMap) {
    let model = &app.model;
    let profile = app.profile();
    if middle.width >= 2 && middle.height >= 2 {
        let body = detail_frame(
            frame,
            middle,
            app,
            hits,
            &profile.name,
            &profile_buttons(profile),
        );
        let width = usize::from(body.width);
        let picked = |row: Row| app.detail && app.current() == Some(row);
        let mut lines = vec![
            Line::from([vec![Span::raw(" ")], status_spans(model, profile)].concat()),
            Line::default(),
            heading(model, "CONNECTIONS", ""),
        ];
        for at in 0..profile.conns.len() {
            lines.push(conn_line(
                model,
                profile,
                at,
                picked(Row::Conn(at)),
                width,
                true,
            ));
        }
        lines.push(Line::default());
        lines.push(heading(model, "AGENTS", ""));
        if profile.agents.is_empty() {
            lines.push(Line::styled(
                "   none: a connects one",
                style(model, Role::Muted),
            ));
        }
        for at in 0..profile.agents.len() {
            lines.push(agent_line(
                model,
                profile,
                at,
                picked(Row::Agent(at)),
                width,
                true,
            ));
        }
        frame.render_widget(Paragraph::new(lines), body);
    }
    if right.width < 2 || right.height < 2 {
        return;
    }
    let row = app.current().unwrap_or(Row::Conn(0));
    match row {
        Row::Conn(at) => {
            let (conn, access) = &profile.conns[at];
            let conn = &CONNS[*conn];
            let buttons = vec![
                Button::new(
                    KeyCode::Char('w'),
                    if access.is_some() {
                        "Change writes…"
                    } else {
                        "Allow writes…"
                    },
                )
                .enabled_if(conn.writable(), "production never takes agents' writes"),
                Button::new(KeyCode::Char('r'), "Revoke")
                    .enabled_if(access.is_some(), "it is read only"),
            ];
            let block = dexo_tui::render::pane_block(model, conn.name, false);
            let inner = block.inner(right);
            frame.render_widget(block, right);
            let used = widgets::action_bar(frame, inner, model, hits, &buttons, None);
            let body = Rect::new(
                inner.x,
                inner.y + used + 1,
                inner.width,
                inner.height.saturating_sub(used + 1),
            );
            use widgets::FieldRow::Field as F;
            let mut rows = vec![
                F("Environment", conn.env.into()),
                F("Reads", reads_words(profile).into()),
                F("Sees", sees_words(profile.sees, &profile.tables)),
            ];
            match access {
                Some(access) => rows.extend([
                    F("Writes", kinds_words(&access.kinds)),
                    F("Asks", "before each write, up to 2 min".into()),
                    F("Left", access.left.clone()),
                ]),
                None if conn.writable() => rows.push(F("Writes", "none: read only".into())),
                None => rows.push(F("Writes", "never: production".into())),
            }
            frame.render_widget(
                Paragraph::new(widgets::field_lines(model, &rows, body.width)),
                body,
            );
        }
        Row::Agent(at) => {
            let agent = profile.agents[at];
            let client = &CLIENTS[agent];
            let buttons = vec![
                Button::new(KeyCode::Char('a'), "Set up again…"),
                Button::new(KeyCode::Char('y'), "Copy command"),
            ];
            let block = dexo_tui::render::pane_block(model, client.name, false);
            let inner = block.inner(right);
            frame.render_widget(block, right);
            let used = widgets::action_bar(frame, inner, model, hits, &buttons, None);
            let body = Rect::new(
                inner.x,
                inner.y + used + 1,
                inner.width,
                inner.height.saturating_sub(used + 1),
            );
            let lines = vec![
                field(
                    model,
                    "Status",
                    vec![Span::raw(client_state(agent, &app.profiles))],
                ),
                field(model, "File", vec![Span::raw(client.file)]),
                field(model, "Skill", vec![Span::raw("written")]),
            ];
            frame.render_widget(Paragraph::new(lines), body);
        }
    }
}

fn draw_form(frame: &mut Frame, area: Rect, app: &App, hits: &mut HitMap) {
    let model = &app.model;
    match app.form.as_ref() {
        Some(Form::Profile(form)) => form.draw(frame, area, model, hits, &app.profiles),
        Some(Form::Writes(form)) => {
            form.draw(frame, area, model, hits, &app.profiles[form.profile])
        }
        Some(Form::Agent(form)) => form.draw(frame, area, model, hits, &app.profiles),
        None => {}
    }
}

fn hints(app: &App) -> String {
    if app.form.is_some() {
        return String::new();
    }
    let looks = "F1-F3 looks";
    if !app.detail {
        return format!(
            "Up/Down pick  Tab its details  n new  ⏎ edit  w writes  a agent  R stop all writes  {looks}"
        );
    }
    match (app.look, app.current()) {
        (_, Some(Row::Conn(_))) => {
            format!("Up/Down walk  w writes  r revoke  Esc the list  {looks}")
        }
        (_, Some(Row::Agent(_))) => {
            format!("Up/Down walk  a set up again  y copy its command  Esc the list  {looks}")
        }
        _ => format!("a connect an agent  Esc the list  {looks}"),
    }
}

fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let model_base = app.model.theme.base(app.model.capabilities);
    frame.render_widget(Block::new().style(model_base), area);
    app.model.sections[Screen::Agents.index()] = if app.detail || app.form.is_some() {
        Section::Detail
    } else {
        Section::List
    };
    let mut hits = HitMap::default();
    let [top, about, _, views, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    let model = &app.model;
    let mut spans = vec![
        Span::styled(
            " Agents → Profiles",
            style(model, Role::Foreground).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · three looks   ", style(model, Role::Muted)),
    ];
    for (look, key, name) in [
        (Look::Sheet, "F1", "Sheet"),
        (Look::Tabs, "F2", "Tabs"),
        (Look::Columns, "F3", "Columns"),
    ] {
        spans.push(if app.look == look {
            Span::styled(
                format!(" {key} {name} "),
                panel::on(model, Role::OnFocus, Role::Focus).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!(" {key} {name} "), style(model, Role::Muted))
        });
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), top);
    frame.render_widget(
        Paragraph::new(Line::styled(
            format!("{}x{}  Ctrl+Q quit ", area.width, area.height),
            style(model, Role::Muted),
        ))
        .alignment(Alignment::Right),
        top,
    );
    let source = match app.look {
        Look::Sheet => {
            "All of a profile on one page: its status, reads and sees, then its connections and agents to walk."
        }
        Look::Tabs => {
            "lazydb's principal view: the profile in a line, its connections and its agents on tabs (←/→)."
        }
        Look::Columns => {
            "lazygit's panels: profiles, the picked one's connections and agents, then the picked row in full."
        }
    };
    frame.render_widget(
        Paragraph::new(Line::styled(
            format!(" {source}"),
            style(model, Role::Muted).add_modifier(Modifier::ITALIC),
        )),
        about,
    );
    dexo_tui::screen::views_bar(
        frame,
        views,
        model,
        &mut hits,
        &[
            ("Approvals 1".into(), false),
            ("Activity".into(), false),
            ("Profiles".into(), true),
        ],
    );
    match app.look {
        Look::Sheet | Look::Tabs => {
            let (list, detail) =
                dexo_tui::screen::list_and_detail(model, &mut hits, body, app.profiles.len());
            draw_list(frame, list, app, &mut hits);
            if app.form.is_some() {
                draw_form(frame, detail, app, &mut hits);
            } else if app.look == Look::Sheet {
                sheet(frame, detail, app, &mut hits);
            } else {
                tabs(frame, detail, app, &mut hits);
            }
        }
        Look::Columns => {
            let (list, rest) = if body.width >= 110 {
                let [list, rest] =
                    Layout::horizontal([Constraint::Percentage(26), Constraint::Min(0)])
                        .areas(body);
                (list, rest)
            } else {
                dexo_tui::screen::list_and_detail(model, &mut hits, body, app.profiles.len())
            };
            draw_list(frame, list, app, &mut hits);
            if app.form.is_some() {
                draw_form(frame, rest, app, &mut hits);
            } else if rest.width >= 80 {
                let [middle, right] =
                    Layout::horizontal([Constraint::Percentage(45), Constraint::Min(0)])
                        .areas(rest);
                columns(frame, middle, right, app, &mut hits);
            } else {
                let [middle, right] =
                    Layout::vertical([Constraint::Percentage(55), Constraint::Min(0)]).areas(rest);
                columns(frame, middle, right, app, &mut hits);
            }
        }
    }
    match &app.notice {
        Some((text, role, at)) if at.elapsed() < Duration::from_secs(6) => frame.render_widget(
            Paragraph::new(Line::styled(
                format!(" {text}"),
                style(model, *role).add_modifier(Modifier::BOLD),
            )),
            bottom,
        ),
        _ => frame.render_widget(
            Paragraph::new(Line::styled(
                format!(" Agents  {}", hints(app)),
                style(model, Role::Muted),
            )),
            bottom,
        ),
    }
}

fn main() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new();
    let result = (|| -> std::io::Result<()> {
        loop {
            terminal.draw(|frame| draw(frame, &mut app))?;
            if !event::poll(Duration::from_millis(250))? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('q' | 'c'))
            {
                return Ok(());
            }
            match key.code {
                KeyCode::F(1) => app.look = Look::Sheet,
                KeyCode::F(2) => app.look = Look::Tabs,
                KeyCode::F(3) => app.look = Look::Columns,
                _ => app.key(key),
            }
            app.row = app.row.min(app.rows().len().saturating_sub(1));
        }
    })();
    ratatui::restore();
    result
}
