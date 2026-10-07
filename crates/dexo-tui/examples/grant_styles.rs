//! New Grant drawn four ways, with the same fields and Dexo's own colours, to choose one
//! before the real form is built:
//!
//!     cargo run -p dexo-tui --example grant_styles
//!
//! F1 Panel follows lazydb's profile form, F2 Boxes sqlit's connection form, F3 Steps
//! tabiew's pickers ending on lazydb's review, and F4 v4 is F1 with F2's dropdowns and
//! boxed inputs. Nothing is saved: Create only says what the
//! grant would be.

use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use dexo_tui::capabilities::TerminalCapabilities;
use dexo_tui::theme::{DEFAULT_ACCENT, Mode, Role, Theme, theme_for};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

// The grant, shared by the three looks: switching keeps what was chosen.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Env {
    Production,
    Staging,
    Development,
}

impl Env {
    fn word(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Staging => "staging",
            Self::Development => "development",
        }
    }

    fn role(self) -> Role {
        match self {
            Self::Production => Role::Production,
            Self::Staging => Role::Staging,
            Self::Development => Role::Development,
        }
    }
}

struct Connection {
    name: &'static str,
    env: Env,
}

const CONNECTIONS: [Connection; 3] = [
    Connection {
        name: "pg-commerce",
        env: Env::Production,
    },
    Connection {
        name: "pg-staging",
        env: Env::Staging,
    },
    Connection {
        name: "sqlite-local",
        env: Env::Development,
    },
];

struct Profile {
    name: &'static str,
    about: &'static str,
    connections: &'static [usize],
}

const PROFILES: [Profile; 3] = [
    Profile {
        name: "claude-code",
        about: "Claude Code in this project",
        connections: &[0, 1],
    },
    Profile {
        name: "cursor",
        about: "Cursor's agent",
        connections: &[1, 2],
    },
    Profile {
        name: "ci-bot",
        about: "the nightly checks",
        connections: &[1],
    },
];

struct Capability {
    name: &'static str,
    about: &'static str,
    tools: &'static [&'static str],
}

const CAPABILITIES: [Capability; 3] = [
    Capability {
        name: "data_write",
        about: "change rows",
        tools: &[
            "data_insert",
            "data_update",
            "data_delete",
            "data_execute_sql",
        ],
    },
    Capability {
        name: "ddl",
        about: "change tables and schemas",
        tools: &["schema_apply_ddl"],
    },
    Capability {
        name: "admin",
        about: "cancel queries, end sessions",
        tools: &["admin_cancel_query", "admin_terminate_session"],
    },
];

const EXPIRES: [(&str, &str); 5] = [
    ("15m", "15 minutes"),
    ("1h", "1 hour"),
    ("2h", "2 hours"),
    ("8h", "8 hours"),
    ("24h", "24 hours"),
];

/// What the catalog would offer for the objects a grant covers.
const OBJECTS: [&str; 8] = [
    "app.public.orders",
    "app.public.order_items",
    "app.public.customers",
    "app.public.*",
    "app.billing.invoices",
    "app.billing.payments",
    "app.billing.*",
    "app.*.*",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Profile,
    Connection,
    Capability,
    Tools,
    Objects,
    Expires,
    Ask,
    AskSecs,
    Confirm,
}

struct Grant {
    profile: usize,
    /// Among the profile's connections.
    connection: usize,
    capability: usize,
    tools: Vec<bool>,
    objects: String,
    expires: usize,
    ask: bool,
    ask_secs: String,
    confirm: String,
}

impl Grant {
    fn new() -> Self {
        Self {
            profile: 0,
            connection: 0,
            capability: 0,
            tools: vec![true, true, false, false],
            objects: "app.public.orders".into(),
            expires: 0,
            ask: false,
            ask_secs: "120".into(),
            confirm: String::new(),
        }
    }

    fn connections(&self) -> &'static [usize] {
        PROFILES[self.profile].connections
    }

    fn connection(&self) -> &'static Connection {
        &CONNECTIONS[self.connections()[self.connection]]
    }

    fn set_profile(&mut self, profile: usize) {
        self.profile = profile;
        self.connection = 0;
    }

    fn set_capability(&mut self, capability: usize) {
        self.capability = capability;
        self.tools = vec![true; CAPABILITIES[capability].tools.len()];
    }

    fn chosen_tools(&self) -> Vec<&'static str> {
        CAPABILITIES[self.capability]
            .tools
            .iter()
            .zip(&self.tools)
            .filter(|(_, on)| **on)
            .map(|(tool, _)| *tool)
            .collect()
    }

    fn allows(&self) -> String {
        format!(
            "{} on {} in {}",
            self.chosen_tools().join(", "),
            self.objects.trim(),
            self.connection().name
        )
    }

    fn how(&self) -> String {
        if self.ask {
            format!("each write waits for you, up to {} s", self.ask_secs.trim())
        } else {
            "one write, then it is spent".into()
        }
    }

    fn lasts(&self) -> String {
        format!("{} · ends in {}", self.how(), EXPIRES[self.expires].1)
    }

    /// What stops Create, on the field to fix.
    fn problem(&self) -> Option<(Field, String)> {
        if self.chosen_tools().is_empty() {
            return Some((Field::Tools, "pick at least one tool".into()));
        }
        if let Some(problem) = objects_problem(&self.objects) {
            return Some((Field::Objects, problem));
        }
        if self.ask
            && !self
                .ask_secs
                .trim()
                .parse::<u32>()
                .is_ok_and(|secs| (1..=3600).contains(&secs))
        {
            return Some((
                Field::AskSecs,
                "a whole number of seconds, 1 to 3600".into(),
            ));
        }
        let typed = self.confirm.trim();
        if typed != self.connection().name && typed != self.objects.trim() {
            return Some((
                Field::Confirm,
                format!("type {} to confirm", self.connection().name),
            ));
        }
        None
    }
}

fn objects_problem(objects: &str) -> Option<String> {
    let objects = objects.trim();
    if objects.is_empty() {
        return Some("name the objects, as app.public.orders".into());
    }
    let parts: Vec<&str> = objects.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || (part.contains('*') && *part != "*"))
    {
        return Some("db.schema.table, each part a name or a lone *".into());
    }
    None
}

fn step(grant: &mut Grant, field: Field, delta: isize) {
    let wrap = |at: usize, len: usize| (at as isize + delta).rem_euclid(len as isize) as usize;
    match field {
        Field::Profile => grant.set_profile(wrap(grant.profile, PROFILES.len())),
        Field::Connection => {
            grant.connection = wrap(grant.connection, grant.connections().len());
        }
        Field::Capability => grant.set_capability(wrap(grant.capability, CAPABILITIES.len())),
        Field::Expires => grant.expires = wrap(grant.expires, EXPIRES.len()),
        _ => {}
    }
}

/// A typed character or Backspace, into `text`.
fn type_into(text: &mut String, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(ch)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            text.push(ch);
            true
        }
        KeyCode::Backspace => text.pop().is_some(),
        _ => false,
    }
}

/// Where `query`'s letters are in `name`, in order and ignoring case; none when one is
/// missing.
fn matched(name: &str, query: &str) -> Option<Vec<usize>> {
    let mut found = Vec::new();
    let mut letters = query.chars().map(|ch| ch.to_ascii_lowercase()).peekable();
    for (at, ch) in name.chars().enumerate() {
        if letters.peek() == Some(&ch.to_ascii_lowercase()) {
            found.push(at);
            letters.next();
        }
    }
    letters.peek().is_none().then_some(found)
}

enum Outcome {
    Stay,
    Create,
    Cancel,
}

// Colours, from Dexo's theme.

struct Ui {
    theme: Theme,
    caps: TerminalCapabilities,
}

impl Ui {
    fn fg(&self, role: Role) -> Style {
        self.theme.style(role, self.caps)
    }

    fn bold(&self, role: Role) -> Style {
        self.fg(role).add_modifier(Modifier::BOLD)
    }

    /// `fg` on `bg`; reversed when the terminal has no colour.
    fn on(&self, fg: Role, bg: Role) -> Style {
        match self.theme.color(bg, self.caps) {
            Some(color) => self.fg(fg).bg(color),
            None => Style::default().add_modifier(Modifier::REVERSED),
        }
    }

    fn base(&self) -> Style {
        let style = self.fg(Role::Foreground);
        match self.theme.color(Role::Background, self.caps) {
            Some(color) => style.bg(color),
            None => style,
        }
    }

    /// `key description` pairs, the key in the accent: lazydb's hint row.
    fn hints(&self, pairs: &[(&str, &str)]) -> Line<'static> {
        let mut spans = Vec::new();
        for (index, (key, what)) in pairs.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw("   "));
            }
            spans.push(Span::styled(key.to_string(), self.bold(Role::Focus)));
            spans.push(Span::styled(format!(" {what}"), self.fg(Role::Muted)));
        }
        Line::from(spans)
    }

    /// `[   Label ]`, or `[ > Label ]` focused: the marker keeps its room either way.
    fn button(&self, label: &str, focused: bool, tone: Role, primary: bool) -> Span<'static> {
        if focused {
            Span::styled(
                format!("[ > {label} ]"),
                self.on(Role::OnFocus, tone).add_modifier(Modifier::BOLD),
            )
        } else if primary {
            Span::styled(format!("[   {label} ]"), self.bold(tone))
        } else {
            Span::styled(format!("[   {label} ]"), self.fg(Role::Muted))
        }
    }
}

fn centered(area: Rect, width: u16) -> Rect {
    let width = width.min(area.width);
    Rect {
        x: area.x + (area.width - width) / 2,
        width,
        ..area
    }
}

fn cursor_at(frame: &mut Frame, area: Rect, x: u16, y: u16) {
    if x < area.right() && y < area.bottom() {
        frame.set_cursor_position((x, y));
    }
}

// F1 Panel: lazydb's profile form. A label column with a `›` on the focused row, choices
// side by side with the chosen one filled, `[x]` boxes, then a button row and its hints.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Field(Field),
    Cancel,
    Create,
}

#[derive(Default)]
struct Panel {
    focus: usize,
    tool: usize,
    error: Option<(Field, String)>,
}

const PANEL_LABEL: usize = 16;

impl Panel {
    fn slots(grant: &Grant) -> Vec<Slot> {
        let mut slots: Vec<Slot> = [
            Field::Profile,
            Field::Connection,
            Field::Capability,
            Field::Tools,
            Field::Objects,
            Field::Expires,
            Field::Ask,
        ]
        .map(Slot::Field)
        .to_vec();
        if grant.ask {
            slots.push(Slot::Field(Field::AskSecs));
        }
        slots.extend([Slot::Field(Field::Confirm), Slot::Cancel, Slot::Create]);
        slots
    }

    fn slot(&self, grant: &Grant) -> Slot {
        let slots = Self::slots(grant);
        slots[self.focus.min(slots.len() - 1)]
    }

    fn fail(&mut self, field: Field, message: String, grant: &Grant) {
        if let Some(at) = Self::slots(grant)
            .iter()
            .position(|slot| *slot == Slot::Field(field))
        {
            self.focus = at;
        }
        self.error = Some((field, message));
    }

    fn key(&mut self, key: KeyEvent, grant: &mut Grant) -> Outcome {
        let slots = Self::slots(grant);
        let slot = self.slot(grant);
        match key.code {
            KeyCode::Tab | KeyCode::Down => {
                self.focus = (self.focus + 1) % slots.len();
                return Outcome::Stay;
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.focus = (self.focus + slots.len() - 1) % slots.len();
                return Outcome::Stay;
            }
            KeyCode::Esc => return Outcome::Cancel,
            KeyCode::Enter if slot == Slot::Cancel => return Outcome::Cancel,
            KeyCode::Enter => return Outcome::Create,
            _ => {}
        }
        match slot {
            Slot::Cancel | Slot::Create => {
                if matches!(key.code, KeyCode::Left | KeyCode::Right) {
                    let other = if slot == Slot::Cancel {
                        Slot::Create
                    } else {
                        Slot::Cancel
                    };
                    self.focus = slots
                        .iter()
                        .position(|at| *at == other)
                        .unwrap_or(self.focus);
                }
            }
            Slot::Field(field) => {
                let delta = match key.code {
                    KeyCode::Left => -1,
                    KeyCode::Right | KeyCode::Char(' ') => 1,
                    _ => 0,
                };
                let last_tool = grant.tools.len().saturating_sub(1);
                match field {
                    Field::Profile | Field::Connection | Field::Capability | Field::Expires => {
                        if delta != 0 {
                            step(grant, field, delta);
                        }
                    }
                    Field::Tools => match key.code {
                        KeyCode::Left => self.tool = self.tool.min(last_tool).saturating_sub(1),
                        KeyCode::Right => self.tool = (self.tool + 1).min(last_tool),
                        KeyCode::Char(' ') => {
                            if let Some(on) = grant.tools.get_mut(self.tool.min(last_tool)) {
                                *on = !*on;
                            }
                        }
                        _ => {}
                    },
                    Field::Ask => {
                        if key.code == KeyCode::Char(' ') {
                            grant.ask = !grant.ask;
                        }
                    }
                    Field::Objects => {
                        type_into(&mut grant.objects, key);
                    }
                    Field::AskSecs => {
                        type_into(&mut grant.ask_secs, key);
                    }
                    Field::Confirm => {
                        type_into(&mut grant.confirm, key);
                    }
                }
                self.error = None;
            }
        }
        Outcome::Stay
    }

    fn label(ui: &Ui, focused: bool, label: &str) -> Vec<Span<'static>> {
        let marker = if focused { "›" } else { " " };
        vec![
            Span::styled(format!(" {marker} "), ui.bold(Role::Focus)),
            Span::styled(
                format!("{label:<PANEL_LABEL$}"),
                if focused {
                    ui.bold(Role::Focus)
                } else {
                    ui.fg(Role::Muted)
                },
            ),
        ]
    }

    /// The options side by side, the chosen one filled.
    fn segmented(ui: &Ui, options: &[&str], chosen: usize, focused: bool) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for (index, option) in options.iter().enumerate() {
            let style = match (index == chosen, focused) {
                (true, true) => ui
                    .on(Role::OnFocus, Role::Focus)
                    .add_modifier(Modifier::BOLD),
                (true, false) => ui.on(Role::OnSelection, Role::Selection),
                _ => ui.fg(Role::Muted),
            };
            spans.push(Span::styled(format!(" {option} "), style));
            spans.push(Span::raw(" "));
        }
        spans
    }

    fn section(ui: &Ui, title: &str) -> Line<'static> {
        Line::styled(format!(" {title}"), ui.bold(Role::Border))
    }

    fn draw(&self, frame: &mut Frame, area: Rect, grant: &Grant, ui: &Ui) {
        // Values start one column into the value column, where a choice's text does.
        let indent = " ".repeat(4 + PANEL_LABEL);
        let body_y = area.y + 1;
        let value_x = area.x + 1 + indent.len() as u16;
        let slot = self.slot(grant);
        let focused = |field: Field| slot == Slot::Field(field);
        let mut lines: Vec<Line> = vec![Line::raw("")];
        let mut cursor = None;
        let mut text_row =
            |lines: &mut Vec<Line>, field: Field, label: &str, text: &str, after: &str| {
                let mut spans = Self::label(ui, focused(field), label);
                spans.push(Span::raw(" "));
                spans.push(Span::styled(text.to_string(), ui.fg(Role::Foreground)));
                spans.push(Span::styled(after.to_string(), ui.fg(Role::Muted)));
                if focused(field) {
                    cursor = Some((
                        value_x + text.chars().count() as u16,
                        body_y + lines.len() as u16,
                    ));
                }
                lines.push(Line::from(spans));
            };

        lines.push(Self::section(ui, "WHO"));
        let names: Vec<&str> = PROFILES.iter().map(|profile| profile.name).collect();
        let mut spans = Self::label(ui, focused(Field::Profile), "Profile");
        spans.extend(Self::segmented(
            ui,
            &names,
            grant.profile,
            focused(Field::Profile),
        ));
        lines.push(Line::from(spans));
        let names: Vec<&str> = grant
            .connections()
            .iter()
            .map(|at| CONNECTIONS[*at].name)
            .collect();
        let mut spans = Self::label(ui, focused(Field::Connection), "Connection");
        spans.extend(Self::segmented(
            ui,
            &names,
            grant.connection,
            focused(Field::Connection),
        ));
        let env = grant.connection().env;
        spans.push(Span::styled(
            format!(" {}", env.word().to_uppercase()),
            ui.bold(env.role()),
        ));
        lines.push(Line::from(spans));

        lines.push(Self::section(ui, "WHAT"));
        let names: Vec<&str> = CAPABILITIES
            .iter()
            .map(|capability| capability.name)
            .collect();
        let mut spans = Self::label(ui, focused(Field::Capability), "Capability");
        spans.extend(Self::segmented(
            ui,
            &names,
            grant.capability,
            focused(Field::Capability),
        ));
        spans.push(Span::styled(
            format!(" {}", CAPABILITIES[grant.capability].about),
            ui.fg(Role::Muted),
        ));
        lines.push(Line::from(spans));
        let tools = CAPABILITIES[grant.capability].tools;
        let at = self.tool.min(tools.len().saturating_sub(1));
        for (row, chunk) in tools.chunks(2).enumerate() {
            let mut spans = if row == 0 {
                let mut spans = Self::label(ui, focused(Field::Tools), "Tools");
                spans.push(Span::raw(" "));
                spans
            } else {
                vec![Span::raw(indent.clone())]
            };
            for (column, tool) in chunk.iter().enumerate() {
                let index = row * 2 + column;
                let on = grant.tools.get(index).copied().unwrap_or(false);
                let picked = focused(Field::Tools) && index == at;
                let style = if picked {
                    ui.on(Role::OnSelection, Role::Selection)
                } else {
                    ui.fg(Role::Foreground)
                };
                spans.push(Span::styled("[", style));
                spans.push(Span::styled(
                    if on { "x" } else { " " },
                    if picked { style } else { ui.bold(Role::Focus) },
                ));
                spans.push(Span::styled(format!("] {tool:<22}"), style));
                spans.push(Span::raw(" "));
            }
            lines.push(Line::from(spans));
        }
        text_row(&mut lines, Field::Objects, "Objects", &grant.objects, "");
        lines.push(Line::styled(
            format!("{indent}db.schema.table, a lone * for any part: app.public.*"),
            ui.fg(Role::Muted),
        ));

        lines.push(Self::section(ui, "HOW LONG"));
        let names: Vec<&str> = EXPIRES.iter().map(|(short, _)| *short).collect();
        let mut spans = Self::label(ui, focused(Field::Expires), "Expires");
        spans.extend(Self::segmented(
            ui,
            &names,
            grant.expires,
            focused(Field::Expires),
        ));
        lines.push(Line::from(spans));
        let mut spans = Self::label(ui, focused(Field::Ask), "Ask each write");
        spans.push(Span::raw(" "));
        let (mark, word) = if grant.ask {
            ("[x]", " On ")
        } else {
            ("[ ]", " Off")
        };
        spans.push(Span::styled(
            format!("{mark}{word}"),
            if focused(Field::Ask) {
                ui.on(Role::OnSelection, Role::Selection)
            } else {
                ui.fg(Role::Foreground)
            },
        ));
        spans.push(Span::styled(
            format!("  {}", grant.how()),
            ui.fg(Role::Muted),
        ));
        lines.push(Line::from(spans));
        if grant.ask {
            text_row(
                &mut lines,
                Field::AskSecs,
                "Wait up to",
                &grant.ask_secs,
                " seconds, 1 to 3600",
            );
        }

        lines.push(Self::section(ui, "CONFIRM"));
        text_row(
            &mut lines,
            Field::Confirm,
            "Type the target",
            &grant.confirm,
            "",
        );
        lines.push(Line::styled(
            format!("{indent}{} or the objects, again", grant.connection().name),
            ui.fg(Role::Muted),
        ));

        lines.push(match &self.error {
            Some((_, message)) => Line::styled(format!(" × {message}"), ui.bold(Role::Error)),
            None => Line::raw(""),
        });
        lines.push(Self::section(ui, "RESULT"));
        lines.push(Line::styled(
            format!("   {}", grant.allows()),
            ui.fg(Role::Foreground),
        ));
        lines.push(Line::styled(
            format!("   {}", grant.lasts()),
            ui.fg(Role::Muted),
        ));
        if env == Env::Production {
            lines.push(Line::styled(
                "   ! writes to a production connection",
                ui.bold(Role::Warning),
            ));
        }
        // As tall as what it holds: the buttons follow the form, not the screen's foot.
        let area = Rect {
            height: (lines.len() as u16 + 5).min(area.height),
            ..area
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(ui.fg(Role::Focus))
            .title(Span::styled(" NEW GRANT ", ui.bold(Role::Focus)))
            .style(ui.base());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [body, _, buttons, hints] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        frame.render_widget(Paragraph::new(lines), body);
        if let Some((x, y)) = cursor {
            cursor_at(frame, body, x, y);
        }

        let tone = if env == Env::Production {
            Role::Warning
        } else {
            Role::Focus
        };
        let row = Line::from(vec![
            ui.button("Cancel", slot == Slot::Cancel, Role::Focus, false),
            Span::raw("  "),
            ui.button("Create grant", slot == Slot::Create, tone, true),
        ])
        .alignment(Alignment::Center);
        frame.render_widget(Paragraph::new(row), buttons);
        frame.render_widget(
            Paragraph::new(ui.hints(&[
                (" Tab", "next"),
                ("←/→", "choose"),
                ("Space", "toggle"),
                ("Enter", "create"),
                ("Esc", "cancel"),
            ])),
            hints,
        );
    }
}

// F2 Boxes: sqlit's connection form. Each field is a box titled by its name, lists that
// matter stay open, dropdowns open under their box, and the keys are in the border:
// there are no buttons.

#[derive(Default)]
struct Boxes {
    focus: usize,
    /// The highlighted row of the dropdown that is open.
    open: Option<usize>,
    tool: usize,
    error: Option<(Field, String)>,
}

impl Boxes {
    fn order(grant: &Grant) -> Vec<Field> {
        let mut order = vec![
            Field::Profile,
            Field::Connection,
            Field::Capability,
            Field::Tools,
            Field::Objects,
            Field::Expires,
            Field::Ask,
        ];
        if grant.ask {
            order.push(Field::AskSecs);
        }
        order.push(Field::Confirm);
        order
    }

    fn field(&self, grant: &Grant) -> Field {
        let order = Self::order(grant);
        order[self.focus.min(order.len() - 1)]
    }

    fn fail(&mut self, field: Field, message: String, grant: &Grant) {
        if let Some(at) = Self::order(grant).iter().position(|at| *at == field) {
            self.focus = at;
        }
        self.open = None;
        self.error = Some((field, message));
    }

    fn options(field: Field, grant: &Grant) -> Vec<String> {
        match field {
            Field::Profile => PROFILES.iter().map(|profile| profile.name.into()).collect(),
            Field::Connection => grant
                .connections()
                .iter()
                .map(|at| {
                    let connection = &CONNECTIONS[*at];
                    format!("{:<14}{}", connection.name, connection.env.word())
                })
                .collect(),
            Field::Expires => EXPIRES
                .iter()
                .map(|(short, long)| format!("{short:<5}{long}"))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn chosen(field: Field, grant: &Grant) -> usize {
        match field {
            Field::Profile => grant.profile,
            Field::Connection => grant.connection,
            Field::Expires => grant.expires,
            _ => 0,
        }
    }

    fn key(&mut self, key: KeyEvent, grant: &mut Grant) -> Outcome {
        let order = Self::order(grant);
        let field = self.field(grant);
        if let Some(at) = self.open {
            let len = Self::options(field, grant).len();
            match key.code {
                KeyCode::Up => self.open = Some(at.saturating_sub(1)),
                KeyCode::Down => self.open = Some((at + 1).min(len - 1)),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    match field {
                        Field::Profile => grant.set_profile(at),
                        Field::Connection => grant.connection = at,
                        Field::Expires => grant.expires = at,
                        _ => {}
                    }
                    self.open = None;
                }
                KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => self.open = None,
                _ => {}
            }
            return Outcome::Stay;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            return Outcome::Create;
        }
        match key.code {
            KeyCode::Tab => {
                self.focus = (self.focus + 1) % order.len();
                return Outcome::Stay;
            }
            KeyCode::BackTab => {
                self.focus = (self.focus + order.len() - 1) % order.len();
                return Outcome::Stay;
            }
            KeyCode::Esc => return Outcome::Cancel,
            _ => {}
        }
        self.error = None;
        let last_tool = grant.tools.len().saturating_sub(1);
        match field {
            Field::Profile | Field::Connection | Field::Expires => {
                if matches!(
                    key.code,
                    KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Down
                ) {
                    self.open = Some(Self::chosen(field, grant));
                }
            }
            Field::Capability => match key.code {
                KeyCode::Up => step(grant, field, -1),
                KeyCode::Down => step(grant, field, 1),
                KeyCode::Enter => self.focus += 1,
                _ => {}
            },
            Field::Tools => match key.code {
                KeyCode::Up => self.tool = self.tool.min(last_tool).saturating_sub(1),
                KeyCode::Down => self.tool = (self.tool + 1).min(last_tool),
                KeyCode::Char(' ') => {
                    if let Some(on) = grant.tools.get_mut(self.tool.min(last_tool)) {
                        *on = !*on;
                    }
                }
                KeyCode::Enter => self.focus += 1,
                _ => {}
            },
            Field::Ask => {
                if matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter) {
                    grant.ask = !grant.ask;
                }
            }
            Field::Objects => {
                if key.code == KeyCode::Enter {
                    self.focus += 1;
                } else {
                    type_into(&mut grant.objects, key);
                }
            }
            Field::AskSecs => {
                if key.code == KeyCode::Enter {
                    self.focus += 1;
                } else {
                    type_into(&mut grant.ask_secs, key);
                }
            }
            Field::Confirm => {
                if key.code == KeyCode::Enter {
                    return Outcome::Create;
                }
                type_into(&mut grant.confirm, key);
            }
        }
        Outcome::Stay
    }

    fn field_block(&self, ui: &Ui, title: &str, field: Field, focus: Field) -> Block<'static> {
        let invalid = self.error.as_ref().is_some_and(|(at, _)| *at == field);
        let (border, label) = if invalid {
            (Role::Error, Role::Error)
        } else if field == focus {
            (Role::Focus, Role::Focus)
        } else {
            (Role::Border, Role::Muted)
        };
        Block::bordered()
            .border_style(ui.fg(border))
            .title(Span::styled(format!(" {title} "), ui.fg(label)))
    }

    /// A closed dropdown: the value, then `▼` at the right edge.
    fn dropdown(ui: &Ui, value: Vec<Span<'static>>, width: u16) -> Line<'static> {
        let used: usize = value.iter().map(|span| span.content.chars().count()).sum();
        let mut spans = vec![Span::raw(" ")];
        spans.extend(value);
        spans.push(Span::raw(
            " ".repeat(usize::from(width).saturating_sub(used + 3)),
        ));
        spans.push(Span::styled("▼", ui.fg(Role::Muted)));
        Line::from(spans)
    }

    fn draw(&self, frame: &mut Frame, area: Rect, grant: &Grant, ui: &Ui) {
        let key = |text: &str| Span::styled(text.to_string(), ui.bold(Role::Focus));
        let say = |text: &str| Span::styled(text.to_string(), ui.fg(Role::Foreground));
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(ui.fg(Role::Focus))
            .title(Span::styled(" New grant ", ui.bold(Role::Focus)))
            .title_bottom(
                Line::from(vec![
                    say(" Create "),
                    key("^s"),
                    say("  Cancel "),
                    key("<esc>"),
                    say(" "),
                ])
                .right_aligned(),
            )
            .style(ui.base());
        let tools = CAPABILITIES[grant.capability].tools;
        let lists = (tools.len() as u16).max(CAPABILITIES.len() as u16) + 2;
        // As tall as its boxes, with the keys in the border right under them.
        let area = Rect {
            height: (lists + 19).min(area.height),
            ..area
        };
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let inner = Rect {
            x: inner.x + 1,
            width: inner.width.saturating_sub(2),
            ..inner
        };
        let [
            who,
            what,
            objects,
            objects_hint,
            how,
            confirm,
            error,
            summary,
            _,
        ] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(lists),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .areas(inner);
        let focus = self.field(grant);
        let halves = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).spacing(1);
        let [profile_area, connection_area] = halves.areas(who);
        let [capability_area, tools_area] = halves.areas(what);

        let block = self.field_block(ui, "Profile", Field::Profile, focus);
        let width = block.inner(profile_area).width;
        frame.render_widget(
            Paragraph::new(Self::dropdown(
                ui,
                vec![say(PROFILES[grant.profile].name)],
                width,
            ))
            .block(block),
            profile_area,
        );
        let block = self.field_block(ui, "Connection", Field::Connection, focus);
        let width = block.inner(connection_area).width;
        let connection = grant.connection();
        frame.render_widget(
            Paragraph::new(Self::dropdown(
                ui,
                vec![
                    say(connection.name),
                    Span::styled(
                        format!("  {}", connection.env.word()),
                        ui.bold(connection.env.role()),
                    ),
                ],
                width,
            ))
            .block(block),
            connection_area,
        );

        // A list left open: its highlighted row is the value.
        let block = self.field_block(ui, "Capability", Field::Capability, focus);
        let width = usize::from(block.inner(capability_area).width);
        let rows: Vec<Line> = CAPABILITIES
            .iter()
            .enumerate()
            .map(|(index, capability)| {
                let text = format!(" {:<12}{}", capability.name, capability.about);
                let text = format!("{text:<width$}");
                if index != grant.capability {
                    Line::styled(text, ui.fg(Role::Muted))
                } else if focus == Field::Capability {
                    Line::styled(text, ui.on(Role::OnFocus, Role::Focus))
                } else {
                    Line::styled(text, ui.on(Role::OnSelection, Role::Selection))
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(rows).block(block), capability_area);

        let block = self.field_block(ui, "Tools", Field::Tools, focus);
        let width = usize::from(block.inner(tools_area).width);
        let at = self.tool.min(tools.len().saturating_sub(1));
        let rows: Vec<Line> = tools
            .iter()
            .enumerate()
            .map(|(index, tool)| {
                let on = grant.tools.get(index).copied().unwrap_or(false);
                let row = if focus == Field::Tools && index == at {
                    ui.on(Role::OnSelection, Role::Selection)
                } else {
                    ui.fg(Role::Foreground)
                };
                Line::from(vec![
                    Span::styled(" ▐", row),
                    Span::styled(if on { "X" } else { " " }, row.patch(ui.bold(Role::Focus))),
                    Span::styled(format!("▌ {tool:<w$}", w = width.saturating_sub(5)), row),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(rows).block(block), tools_area);

        let block = self.field_block(ui, "Objects", Field::Objects, focus);
        let text_area = block.inner(objects);
        frame.render_widget(
            Paragraph::new(Line::from(vec![say(" "), say(&grant.objects)])).block(block),
            objects,
        );
        if focus == Field::Objects {
            cursor_at(
                frame,
                text_area,
                text_area.x + 1 + grant.objects.chars().count() as u16,
                text_area.y,
            );
        }
        frame.render_widget(
            Paragraph::new(Line::styled(
                "  db.schema.table, a lone * for any part: app.public.*",
                ui.fg(Role::Muted),
            )),
            objects_hint,
        );

        let mut columns = vec![Constraint::Length(24), Constraint::Fill(1)];
        if grant.ask {
            columns.push(Constraint::Length(18));
        }
        let parts = Layout::horizontal(columns).spacing(1).split(how);
        let block = self.field_block(ui, "Expires", Field::Expires, focus);
        let width = block.inner(parts[0]).width;
        let (short, long) = EXPIRES[grant.expires];
        frame.render_widget(
            Paragraph::new(Self::dropdown(
                ui,
                vec![
                    say(short),
                    Span::styled(format!("  {long}"), ui.fg(Role::Muted)),
                ],
                width,
            ))
            .block(block),
            parts[0],
        );
        let block = self.field_block(ui, "Ask before each write", Field::Ask, focus);
        let edge = ui.fg(Role::Border);
        let mut switch = vec![Span::raw(" "), Span::styled("▐", edge)];
        if grant.ask {
            switch.push(Span::styled("───", edge));
            switch.push(Span::styled("██", ui.fg(Role::Focus)));
        } else {
            switch.push(Span::styled("██", ui.fg(Role::Muted)));
            switch.push(Span::styled("───", edge));
        }
        switch.push(Span::styled("▌", edge));
        switch.push(if grant.ask {
            Span::styled(" On ", ui.bold(Role::Focus))
        } else {
            Span::styled(" Off", ui.fg(Role::Muted))
        });
        switch.push(Span::styled(
            format!("  {}", grant.how()),
            ui.fg(Role::Muted),
        ));
        frame.render_widget(Paragraph::new(Line::from(switch)).block(block), parts[1]);
        if grant.ask {
            let block = self.field_block(ui, "Wait up to", Field::AskSecs, focus);
            let text_area = block.inner(parts[2]);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    say(" "),
                    say(&grant.ask_secs),
                    Span::styled(" s", ui.fg(Role::Muted)),
                ]))
                .block(block),
                parts[2],
            );
            if focus == Field::AskSecs {
                cursor_at(
                    frame,
                    text_area,
                    text_area.x + 1 + grant.ask_secs.chars().count() as u16,
                    text_area.y,
                );
            }
        }

        let title = format!("Type {} to confirm", connection.name);
        let block = self.field_block(ui, &title, Field::Confirm, focus);
        let text_area = block.inner(confirm);
        frame.render_widget(
            Paragraph::new(Line::from(vec![say(" "), say(&grant.confirm)])).block(block),
            confirm,
        );
        if focus == Field::Confirm {
            cursor_at(
                frame,
                text_area,
                text_area.x + 1 + grant.confirm.chars().count() as u16,
                text_area.y,
            );
        }

        if let Some((_, message)) = &self.error {
            frame.render_widget(
                Paragraph::new(Line::styled(format!(" × {message}"), ui.bold(Role::Error))),
                error,
            );
        }
        let mut lines = vec![
            Line::styled(format!(" ✓ {}", grant.allows()), ui.fg(Role::Success)),
            Line::styled(format!("   {}", grant.lasts()), ui.fg(Role::Muted)),
        ];
        if connection.env == Env::Production {
            lines.push(Line::styled(
                "   ! writes to a production connection",
                ui.bold(Role::Warning),
            ));
        }
        frame.render_widget(Paragraph::new(lines), summary);

        // The open dropdown, over what is under its box.
        if let Some(highlight) = self.open {
            let anchor = match focus {
                Field::Profile => profile_area,
                Field::Connection => connection_area,
                _ => parts[0],
            };
            let options = Self::options(focus, grant);
            let height = (options.len() as u16 + 2).min(frame.area().height);
            let list = Rect {
                y: (anchor.y + 2).min(frame.area().height.saturating_sub(height)),
                height,
                ..anchor
            };
            let width = usize::from(list.width.saturating_sub(2));
            let rows: Vec<Line> = options
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    let text = format!(" {option:<w$}", w = width.saturating_sub(1));
                    if index == highlight {
                        Line::styled(text, ui.on(Role::OnFocus, Role::Focus))
                    } else {
                        Line::styled(text, ui.fg(Role::Foreground))
                    }
                })
                .collect();
            frame.render_widget(Clear, list);
            frame.render_widget(
                Paragraph::new(rows).block(
                    Block::bordered()
                        .border_style(ui.fg(Role::Focus))
                        .style(ui.base()),
                ),
                list,
            );
        }
    }
}

// F3 Steps: tabiew's pickers, one question at a time, each list with a filter joined on
// top; it ends on lazydb's review, where the target is typed to create it.

const STEPS: [&str; 7] = [
    "Profile",
    "Connection",
    "Capability",
    "Tools",
    "Objects",
    "Duration",
    "Review",
];
const REVIEW: usize = 6;

#[derive(Default)]
struct Steps {
    step: usize,
    query: String,
    cursor: usize,
    /// The suggestion picked with the arrows, on the objects step.
    suggestion: Option<usize>,
    /// On the review: the confirmation, then Create, then Back.
    review: usize,
    error: Option<String>,
}

impl Steps {
    /// The step's rows that pass the filter: their index, name, detail, detail colour
    /// and the letters that matched.
    fn choices(&self, grant: &Grant) -> Vec<(usize, &'static str, String, Role, Vec<usize>)> {
        let all: Vec<(usize, &'static str, String, Role)> = match self.step {
            0 => PROFILES
                .iter()
                .enumerate()
                .map(|(at, profile)| (at, profile.name, profile.about.into(), Role::Muted))
                .collect(),
            1 => grant
                .connections()
                .iter()
                .enumerate()
                .map(|(at, index)| {
                    let connection = &CONNECTIONS[*index];
                    (
                        at,
                        connection.name,
                        connection.env.word().into(),
                        connection.env.role(),
                    )
                })
                .collect(),
            2 => CAPABILITIES
                .iter()
                .enumerate()
                .map(|(at, capability)| (at, capability.name, capability.about.into(), Role::Muted))
                .collect(),
            _ => Vec::new(),
        };
        all.into_iter()
            .filter_map(|(at, name, detail, role)| {
                matched(name, &self.query).map(|hits| (at, name, detail, role, hits))
            })
            .collect()
    }

    fn suggestions(grant: &Grant) -> Vec<(&'static str, Vec<usize>)> {
        OBJECTS
            .iter()
            .filter_map(|object| matched(object, grant.objects.trim()).map(|hits| (*object, hits)))
            .collect()
    }

    fn go(&mut self, step: usize, grant: &Grant) {
        self.step = step;
        self.query.clear();
        self.suggestion = None;
        self.review = 0;
        self.error = None;
        self.cursor = match step {
            0 => grant.profile,
            1 => grant.connection,
            2 => grant.capability,
            5 => grant.expires,
            _ => 0,
        };
    }

    fn fail(&mut self, field: Field, message: String, grant: &Grant) {
        let step = match field {
            Field::Profile => 0,
            Field::Connection => 1,
            Field::Capability => 2,
            Field::Tools => 3,
            Field::Objects => 4,
            Field::Expires | Field::Ask | Field::AskSecs => 5,
            Field::Confirm => REVIEW,
        };
        if step != self.step {
            self.go(step, grant);
        }
        self.error = Some(message);
    }

    fn key(&mut self, key: KeyEvent, grant: &mut Grant) -> Outcome {
        match key.code {
            KeyCode::Esc => return Outcome::Cancel,
            KeyCode::BackTab => {
                if self.step > 0 {
                    self.go(self.step - 1, grant);
                }
                return Outcome::Stay;
            }
            _ => {}
        }
        match self.step {
            0..=2 => {
                let choices = self.choices(grant);
                match key.code {
                    KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
                    KeyCode::Down => {
                        self.cursor = (self.cursor + 1).min(choices.len().saturating_sub(1));
                    }
                    KeyCode::Enter => {
                        if let Some((at, ..)) = choices.get(self.cursor) {
                            match self.step {
                                0 => grant.set_profile(*at),
                                1 => grant.connection = *at,
                                _ => grant.set_capability(*at),
                            }
                            self.go(self.step + 1, grant);
                        }
                    }
                    _ => {
                        if type_into(&mut self.query, key) {
                            self.cursor = 0;
                        }
                    }
                }
            }
            3 => {
                let last = grant.tools.len().saturating_sub(1);
                match key.code {
                    KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
                    KeyCode::Down => self.cursor = (self.cursor + 1).min(last),
                    KeyCode::Char(' ') => {
                        if let Some(on) = grant.tools.get_mut(self.cursor.min(last)) {
                            *on = !*on;
                        }
                        self.error = None;
                    }
                    KeyCode::Enter if grant.chosen_tools().is_empty() => {
                        self.error = Some("pick at least one tool".into());
                    }
                    KeyCode::Enter => self.go(4, grant),
                    _ => {}
                }
            }
            4 => {
                let suggestions = Self::suggestions(grant);
                let last = suggestions.len().saturating_sub(1);
                match key.code {
                    KeyCode::Up => self.suggestion = self.suggestion.map(|at| at.saturating_sub(1)),
                    KeyCode::Down if !suggestions.is_empty() => {
                        self.suggestion = Some(self.suggestion.map_or(0, |at| (at + 1).min(last)));
                    }
                    KeyCode::Tab | KeyCode::Enter
                        if self.suggestion.is_some() || key.code == KeyCode::Tab =>
                    {
                        if let Some((object, _)) = suggestions.get(self.suggestion.unwrap_or(0)) {
                            grant.objects = (*object).to_string();
                        }
                        self.suggestion = None;
                    }
                    KeyCode::Enter => match objects_problem(&grant.objects) {
                        Some(problem) => self.error = Some(problem),
                        None => self.go(5, grant),
                    },
                    _ => {
                        if type_into(&mut grant.objects, key) {
                            self.suggestion = None;
                            self.error = None;
                        }
                    }
                }
            }
            5 => match key.code {
                KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
                KeyCode::Down => self.cursor = (self.cursor + 1).min(EXPIRES.len()),
                KeyCode::Char(' ') | KeyCode::Enter if self.cursor == EXPIRES.len() => {
                    grant.ask = !grant.ask;
                }
                KeyCode::Enter => {
                    grant.expires = self.cursor;
                    self.go(REVIEW, grant);
                }
                _ => {}
            },
            _ => match key.code {
                KeyCode::Tab => self.review = (self.review + 1) % 3,
                KeyCode::Left | KeyCode::Right if self.review > 0 => {
                    self.review = 3 - self.review;
                }
                KeyCode::Enter if self.review == 2 => self.go(5, grant),
                KeyCode::Enter => return Outcome::Create,
                _ => {
                    if self.review == 0 && type_into(&mut grant.confirm, key) {
                        self.error = None;
                    }
                }
            },
        }
        Outcome::Stay
    }

    /// The trail of steps: done ones ticked, the current one filled.
    fn trail(&self, ui: &Ui) -> Line<'static> {
        let mut spans = vec![Span::raw(" ")];
        for (index, name) in STEPS.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(" › ", ui.fg(Role::Border)));
            }
            spans.push(if index < self.step {
                Span::styled(format!("✓ {name}"), ui.fg(Role::Success))
            } else if index == self.step {
                Span::styled(
                    format!(" {name} "),
                    ui.on(Role::OnFocus, Role::Focus)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(name.to_string(), ui.fg(Role::Muted))
            });
        }
        Line::from(spans)
    }

    /// What was answered so far, in one line.
    fn so_far(&self, grant: &Grant) -> String {
        let connection = grant.connection();
        let parts = [
            PROFILES[grant.profile].name.to_string(),
            format!("{} ({})", connection.name, connection.env.word()),
            CAPABILITIES[grant.capability].name.to_string(),
            grant.chosen_tools().join(", "),
            grant.objects.trim().to_string(),
            EXPIRES[grant.expires].1.to_string(),
        ];
        let done = self.step.min(parts.len());
        if done == 0 {
            return String::new();
        }
        format!(" So far: {}", parts[..done].join(" → "))
    }

    fn question(&self, grant: &Grant) -> String {
        match self.step {
            0 => " Who gets the grant? ".into(),
            1 => format!(
                " Which connection may {} write to? ",
                PROFILES[grant.profile].name
            ),
            2 => " What may it do? ".into(),
            3 => " Which tools? ".into(),
            4 => " On which objects? ".into(),
            5 => " For how long? ".into(),
            _ => " Review ".into(),
        }
    }

    /// The filter line, a rule joined to the box's sides, then the list under it.
    fn picker(
        frame: &mut Frame,
        area: Rect,
        ui: &Ui,
        title: &str,
        input: (&str, &str),
        rows: Vec<Line<'static>>,
    ) {
        let height = (rows.len() as u16 + 4).min(area.height);
        let area = Rect { height, ..area };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(ui.fg(Role::Focus))
            .title(Span::styled(title.to_string(), ui.bold(Role::Focus)))
            .style(ui.base());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let (text, placeholder) = input;
        let line = if text.is_empty() {
            Line::from(vec![
                Span::styled(" › ", ui.bold(Role::Focus)),
                Span::styled(placeholder.to_string(), ui.fg(Role::Muted)),
            ])
        } else {
            Line::from(vec![
                Span::styled(" › ", ui.bold(Role::Focus)),
                Span::styled(text.to_string(), ui.fg(Role::Foreground)),
            ])
        };
        frame.render_widget(Paragraph::new(line), Rect { height: 1, ..inner });
        cursor_at(
            frame,
            inner,
            inner.x + 3 + text.chars().count() as u16,
            inner.y,
        );
        if area.height > 3 {
            let rule = format!(
                "├{}┤",
                "─".repeat(usize::from(area.width.saturating_sub(2)))
            );
            frame.render_widget(
                Paragraph::new(Line::styled(rule, ui.fg(Role::Focus))),
                Rect {
                    y: area.y + 2,
                    height: 1,
                    ..area
                },
            );
            frame.render_widget(
                Paragraph::new(rows),
                Rect {
                    y: inner.y + 2,
                    height: inner.height.saturating_sub(2),
                    ..inner
                },
            );
        }
    }

    fn draw(&self, frame: &mut Frame, area: Rect, grant: &Grant, ui: &Ui) {
        let [trail, _, body, so_far, hints] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(self.trail(ui)), trail);
        frame.render_widget(
            Paragraph::new(Line::styled(self.so_far(grant), ui.fg(Role::Muted))),
            so_far,
        );
        let body = centered(body, 78);
        let title = self.question(grant);
        let width = usize::from(body.width.saturating_sub(2));
        let row = |highlighted: bool, spans: Vec<Span<'static>>| -> Line<'static> {
            let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
            let mut spans = spans;
            spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
            let line = Line::from(spans);
            if highlighted {
                line.style(ui.on(Role::OnSelection, Role::Selection))
            } else {
                line
            }
        };
        let error_line = |error: &Option<String>| -> Option<Line<'static>> {
            error
                .as_ref()
                .map(|message| Line::styled(format!(" × {message}"), ui.bold(Role::Error)))
        };
        let pairs: &[(&str, &str)] = match self.step {
            0..=2 => {
                let choices = self.choices(grant);
                let mut rows: Vec<Line> = choices
                    .iter()
                    .enumerate()
                    .map(|(index, (_, name, detail, role, hits))| {
                        let mut spans = vec![Span::raw("  ")];
                        for (at, ch) in name.chars().enumerate() {
                            spans.push(Span::styled(
                                ch.to_string(),
                                if hits.contains(&at) {
                                    ui.bold(Role::Focus)
                                } else {
                                    ui.fg(Role::Foreground)
                                },
                            ));
                        }
                        spans.push(Span::raw(" ".repeat(16usize.saturating_sub(name.len()))));
                        spans.push(Span::styled(detail.clone(), ui.fg(*role)));
                        row(index == self.cursor, spans)
                    })
                    .collect();
                if rows.is_empty() {
                    rows.push(Line::styled("  nothing matches", ui.fg(Role::Muted)));
                }
                Self::picker(
                    frame,
                    body,
                    ui,
                    &title,
                    (&self.query, "type to filter"),
                    rows,
                );
                &[
                    ("type", "filter"),
                    ("↑/↓", "choose"),
                    ("Enter", "next"),
                    ("Shift+Tab", "back"),
                    ("Esc", "cancel"),
                ]
            }
            3 => {
                let mut rows: Vec<Line> = CAPABILITIES[grant.capability]
                    .tools
                    .iter()
                    .enumerate()
                    .map(|(index, tool)| {
                        let on = grant.tools.get(index).copied().unwrap_or(false);
                        row(
                            index == self.cursor,
                            vec![
                                Span::raw("  ["),
                                Span::styled(if on { "x" } else { " " }, ui.bold(Role::Focus)),
                                Span::raw(format!("] {tool}")),
                            ],
                        )
                    })
                    .collect();
                rows.extend(error_line(&self.error));
                Self::boxed(frame, body, ui, &title, rows);
                &[
                    ("↑/↓", "move"),
                    ("Space", "toggle"),
                    ("Enter", "next"),
                    ("Shift+Tab", "back"),
                    ("Esc", "cancel"),
                ]
            }
            4 => {
                let mut rows: Vec<Line> = Self::suggestions(grant)
                    .into_iter()
                    .enumerate()
                    .map(|(index, (object, hits))| {
                        let mut spans = vec![Span::raw("  ")];
                        for (at, ch) in object.chars().enumerate() {
                            spans.push(Span::styled(
                                ch.to_string(),
                                if hits.contains(&at) {
                                    ui.bold(Role::Focus)
                                } else {
                                    ui.fg(Role::Foreground)
                                },
                            ));
                        }
                        row(self.suggestion == Some(index), spans)
                    })
                    .collect();
                rows.push(Line::styled(
                    "  db.schema.table, a lone * for any part",
                    ui.fg(Role::Muted),
                ));
                rows.extend(error_line(&self.error));
                Self::picker(
                    frame,
                    body,
                    ui,
                    &title,
                    (&grant.objects, "app.public.orders"),
                    rows,
                );
                &[
                    ("type", "objects"),
                    ("↑/↓", "suggestion"),
                    ("Tab", "complete"),
                    ("Enter", "next"),
                    ("Shift+Tab", "back"),
                ]
            }
            5 => {
                let mut rows: Vec<Line> = EXPIRES
                    .iter()
                    .enumerate()
                    .map(|(index, (short, long))| {
                        row(
                            index == self.cursor,
                            vec![
                                Span::raw(format!("  {short:<6}")),
                                Span::styled(long.to_string(), ui.fg(Role::Muted)),
                            ],
                        )
                    })
                    .collect();
                rows.push(Line::styled(
                    format!("  {}", "─".repeat(width.saturating_sub(4))),
                    ui.fg(Role::Border),
                ));
                rows.push(row(
                    self.cursor == EXPIRES.len(),
                    vec![
                        Span::raw("  ["),
                        Span::styled(if grant.ask { "x" } else { " " }, ui.bold(Role::Focus)),
                        Span::raw("] Ask before each write   "),
                        Span::styled(grant.how(), ui.fg(Role::Muted)),
                    ],
                ));
                Self::boxed(frame, body, ui, &title, rows);
                &[
                    ("↑/↓", "choose"),
                    ("Enter", "next"),
                    ("Space", "ask toggle"),
                    ("Shift+Tab", "back"),
                    ("Esc", "cancel"),
                ]
            }
            _ => {
                self.review(frame, body, grant, ui);
                &[
                    ("type", "to confirm"),
                    ("Tab", "next"),
                    ("Enter", "create"),
                    ("Shift+Tab", "back"),
                    ("Esc", "cancel"),
                ]
            }
        };
        frame.render_widget(Paragraph::new(ui.hints(pairs)), hints);
    }

    /// A list with no filter on top.
    fn boxed(frame: &mut Frame, area: Rect, ui: &Ui, title: &str, rows: Vec<Line<'static>>) {
        let area = Rect {
            height: (rows.len() as u16 + 2).min(area.height),
            ..area
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(ui.fg(Role::Focus))
            .title(Span::styled(title.to_string(), ui.bold(Role::Focus)))
            .style(ui.base());
        frame.render_widget(Paragraph::new(rows).block(block), area);
    }

    /// lazydb's review: what will be granted, the risk, the target typed again, then the
    /// buttons.
    fn review(&self, frame: &mut Frame, area: Rect, grant: &Grant, ui: &Ui) {
        let connection = grant.connection();
        let field = |label: &str, value: Vec<Span<'static>>| -> Line<'static> {
            let mut spans = vec![Span::styled(format!(" {label:<13}"), ui.fg(Role::Muted))];
            spans.extend(value);
            Line::from(spans)
        };
        let say = |text: String| Span::styled(text, ui.fg(Role::Foreground));
        let mut lines = vec![
            Line::raw(""),
            field("Profile", vec![say(PROFILES[grant.profile].name.into())]),
            field(
                "Connection",
                vec![
                    say(connection.name.into()),
                    Span::styled(
                        format!("  {}", connection.env.word().to_uppercase()),
                        ui.bold(connection.env.role()),
                    ),
                ],
            ),
            field("Allows", vec![say(grant.chosen_tools().join(", "))]),
            field("On", vec![say(grant.objects.trim().into())]),
            field("Lasts", vec![say(grant.lasts())]),
            Line::raw(""),
        ];
        if connection.env == Env::Production {
            lines.push(field(
                "Risk",
                vec![Span::styled(
                    "writes to a production connection".to_string(),
                    ui.bold(Role::Warning),
                )],
            ));
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(
            format!(" Type {} to create it:", connection.name),
            ui.fg(Role::Foreground),
        ));
        let input_row = lines.len();
        lines.push(Line::from(vec![
            Span::styled(
                " > ",
                if self.review == 0 {
                    ui.bold(Role::Focus)
                } else {
                    ui.fg(Role::Muted)
                },
            ),
            say(grant.confirm.clone()),
        ]));
        lines.push(match &self.error {
            Some(message) => Line::styled(format!(" × {message}"), ui.bold(Role::Error)),
            None => Line::raw(""),
        });
        let tone = if connection.env == Env::Production {
            Role::Warning
        } else {
            Role::Focus
        };
        lines.push(
            Line::from(vec![
                ui.button("Back", self.review == 2, Role::Focus, false),
                Span::raw("  "),
                ui.button("Create grant", self.review == 1, tone, true),
            ])
            .alignment(Alignment::Center),
        );
        let area = Rect {
            height: (lines.len() as u16 + 3).min(area.height),
            ..area
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(ui.fg(Role::Focus))
            .title(Span::styled(" Review ", ui.bold(Role::Focus)))
            .style(ui.base());
        let inner = block.inner(area);
        frame.render_widget(Paragraph::new(lines).block(block), area);
        if self.review == 0 {
            cursor_at(
                frame,
                inner,
                inner.x + 3 + grant.confirm.chars().count() as u16,
                inner.y + input_row as u16,
            );
        }
    }
}

// F4 v4: F1's panel, with F2's dropdowns for the profile and the connection and F2's
// boxed inputs for what is typed. Enter creates only on the Create button; on the other
// rows it toggles, opens or moves on.

#[derive(Default)]
struct Mixed {
    focus: usize,
    tool: usize,
    /// The highlighted row of the dropdown that is open.
    open: Option<usize>,
    error: Option<(Field, String)>,
}

/// Where a box's left edge sits: after ` › ` and the label.
const MIXED_LABEL: usize = 3 + PANEL_LABEL;

impl Mixed {
    fn slot(&self, grant: &Grant) -> Slot {
        let slots = Panel::slots(grant);
        slots[self.focus.min(slots.len() - 1)]
    }

    fn fail(&mut self, field: Field, message: String, grant: &Grant) {
        if let Some(at) = Panel::slots(grant)
            .iter()
            .position(|slot| *slot == Slot::Field(field))
        {
            self.focus = at;
        }
        self.open = None;
        self.error = Some((field, message));
    }

    fn pick(field: Field, at: usize, grant: &mut Grant) {
        match field {
            Field::Profile => grant.set_profile(at),
            Field::Connection => grant.connection = at,
            _ => {}
        }
    }

    fn key(&mut self, key: KeyEvent, grant: &mut Grant) -> Outcome {
        let slots = Panel::slots(grant);
        let slot = self.slot(grant);
        if let (Some(at), Slot::Field(field)) = (self.open, slot) {
            let last = Boxes::options(field, grant).len().saturating_sub(1);
            match key.code {
                KeyCode::Up => self.open = Some(at.saturating_sub(1)),
                KeyCode::Down => self.open = Some((at + 1).min(last)),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    Self::pick(field, at, grant);
                    self.open = None;
                }
                KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => self.open = None,
                _ => {}
            }
            return Outcome::Stay;
        }
        let next = (self.focus + 1) % slots.len();
        let previous = (self.focus + slots.len() - 1) % slots.len();
        // The tools are a grid of two columns: Up and Down move between its rows, and
        // leave it past the first or the last.
        let last_tool = grant.tools.len().saturating_sub(1);
        let tool = self.tool.min(last_tool);
        let in_tools = slot == Slot::Field(Field::Tools);
        match key.code {
            KeyCode::Down if in_tools && tool / 2 < last_tool / 2 => {
                self.tool = (tool + 2).min(last_tool);
                return Outcome::Stay;
            }
            KeyCode::Up if in_tools && tool / 2 > 0 => {
                self.tool = tool - 2;
                return Outcome::Stay;
            }
            KeyCode::Tab | KeyCode::Down => {
                self.focus = next;
                // Come in from above: the grid's first row, in the same column.
                self.tool = tool % 2;
                return Outcome::Stay;
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.focus = previous;
                // Come in from below: the grid's last row, in the same column.
                if key.code == KeyCode::Up {
                    self.tool = (last_tool / 2 * 2 + tool % 2).min(last_tool);
                }
                return Outcome::Stay;
            }
            KeyCode::Esc => return Outcome::Cancel,
            _ => {}
        }
        match slot {
            Slot::Cancel | Slot::Create => match key.code {
                KeyCode::Left | KeyCode::Right => {
                    let other = if slot == Slot::Cancel {
                        Slot::Create
                    } else {
                        Slot::Cancel
                    };
                    self.focus = slots
                        .iter()
                        .position(|at| *at == other)
                        .unwrap_or(self.focus);
                }
                // The only Enter that creates.
                KeyCode::Enter if slot == Slot::Create => return Outcome::Create,
                KeyCode::Enter => return Outcome::Cancel,
                _ => {}
            },
            Slot::Field(field) => {
                self.error = None;
                let last_tool = grant.tools.len().saturating_sub(1);
                match (field, key.code) {
                    (Field::Profile | Field::Connection, KeyCode::Enter | KeyCode::Char(' ')) => {
                        self.open = Some(Boxes::chosen(field, grant));
                    }
                    (Field::Profile | Field::Connection, KeyCode::Left) => step(grant, field, -1),
                    (Field::Profile | Field::Connection, KeyCode::Right) => step(grant, field, 1),
                    (Field::Capability | Field::Expires, KeyCode::Left) => step(grant, field, -1),
                    (Field::Capability | Field::Expires, KeyCode::Right | KeyCode::Char(' ')) => {
                        step(grant, field, 1);
                    }
                    (Field::Tools, KeyCode::Left) => {
                        self.tool = self.tool.min(last_tool).saturating_sub(1);
                    }
                    (Field::Tools, KeyCode::Right) => self.tool = (self.tool + 1).min(last_tool),
                    (Field::Tools, KeyCode::Char(' ') | KeyCode::Enter) => {
                        if let Some(on) = grant.tools.get_mut(self.tool.min(last_tool)) {
                            *on = !*on;
                        }
                    }
                    (Field::Ask, KeyCode::Char(' ') | KeyCode::Enter) => grant.ask = !grant.ask,
                    (_, KeyCode::Enter) => self.focus = next,
                    (Field::Objects, _) => {
                        type_into(&mut grant.objects, key);
                    }
                    (Field::AskSecs, _) => {
                        type_into(&mut grant.ask_secs, key);
                    }
                    (Field::Confirm, _) => {
                        type_into(&mut grant.confirm, key);
                    }
                    _ => {}
                }
            }
        }
        Outcome::Stay
    }

    /// What the keys do on the focused row.
    fn hints(&self, slot: Slot) -> &'static [(&'static str, &'static str)] {
        if self.open.is_some() {
            return &[(" ↑/↓", "choose"), ("Enter", "pick"), ("Esc", "close")];
        }
        match slot {
            Slot::Field(Field::Profile | Field::Connection) => &[
                (" Enter", "open"),
                ("←/→", "change"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            Slot::Field(Field::Capability | Field::Expires) => {
                &[(" ←/→", "choose"), ("Tab", "next"), ("Esc", "cancel")]
            }
            Slot::Field(Field::Tools) => &[
                (" ↑/↓/←/→", "move"),
                ("Space/Enter", "toggle"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            Slot::Field(Field::Ask) => &[
                (" Space/Enter", "toggle"),
                ("Tab", "next"),
                ("Esc", "cancel"),
            ],
            Slot::Field(_) => &[
                (" Enter/Tab", "next"),
                ("Shift+Tab", "back"),
                ("Esc", "cancel"),
            ],
            Slot::Create => &[
                (" Enter", "create the grant"),
                ("←/→", "switch"),
                ("Esc", "cancel"),
            ],
            Slot::Cancel => &[(" Enter", "cancel"), ("←/→", "switch")],
        }
    }

    /// A box in the value column, as three lines: its top, the label beside its
    /// middle, and its foot with `foot` at the right.
    fn boxed(
        ui: &Ui,
        label: Vec<Span<'static>>,
        width: usize,
        content: Vec<Span<'static>>,
        foot: &str,
        border: Role,
    ) -> [Line<'static>; 3] {
        let indent: usize = label.iter().map(|span| span.content.chars().count()).sum();
        let edge = ui.fg(border);
        let pad = " ".repeat(indent);
        let inside = width.saturating_sub(2);
        let top = Line::from(vec![
            Span::raw(pad.clone()),
            Span::styled(format!("┌{}┐", "─".repeat(inside)), edge),
        ]);
        let used: usize = content
            .iter()
            .map(|span| span.content.chars().count())
            .sum();
        let mut middle = label;
        middle.push(Span::styled("│ ", edge));
        middle.extend(content);
        middle.push(Span::raw(" ".repeat(inside.saturating_sub(used + 1))));
        middle.push(Span::styled("│", edge));
        let foot = if foot.is_empty() {
            String::new()
        } else {
            format!(" {foot} ")
        };
        let rule = inside.saturating_sub(foot.chars().count() + 1);
        let bottom = Line::from(vec![
            Span::raw(pad),
            Span::styled(format!("└{}", "─".repeat(rule)), edge),
            Span::styled(foot, ui.fg(Role::Muted)),
            Span::styled("─┘", edge),
        ]);
        [top, Line::from(middle), bottom]
    }

    fn draw(&self, frame: &mut Frame, area: Rect, grant: &Grant, ui: &Ui) {
        let slot = self.slot(grant);
        let focused = |field: Field| slot == Slot::Field(field);
        let invalid = |field: Field| self.error.as_ref().is_some_and(|(at, _)| *at == field);
        let border = |field: Field| {
            if invalid(field) {
                Role::Error
            } else if focused(field) {
                Role::Focus
            } else {
                Role::Border
            }
        };
        let say = |text: &str| Span::styled(text.to_string(), ui.fg(Role::Foreground));
        let width = usize::from(area.width.saturating_sub(2)).saturating_sub(MIXED_LABEL + 1);
        // A dropdown is as wide as a name needs, a typed field as the form.
        let narrow = width.min(44);
        let mut lines: Vec<Line> = vec![Line::raw("")];
        // The focused row's line, to keep it in view; the typed field's cursor; the line
        // the open dropdown hangs from.
        let mut focus_line = 0;
        let mut cursor: Option<(usize, usize)> = None;
        let mut hang: Option<usize> = None;

        lines.push(Panel::section(ui, "WHO"));
        for (field, name) in [
            (Field::Profile, "Profile"),
            (Field::Connection, "Connection"),
        ] {
            let mut value = match field {
                Field::Profile => vec![say(PROFILES[grant.profile].name)],
                _ => {
                    let connection = grant.connection();
                    vec![
                        say(connection.name),
                        Span::styled(
                            format!("  {}", connection.env.word()),
                            ui.bold(connection.env.role()),
                        ),
                    ]
                }
            };
            // The value, then `▼` against the right edge.
            let used: usize = value.iter().map(|span| span.content.chars().count()).sum();
            value.push(Span::raw(" ".repeat(narrow.saturating_sub(used + 5))));
            value.push(Span::styled("▼ ", ui.fg(Role::Muted)));
            if focused(field) {
                focus_line = lines.len() + 1;
                if self.open.is_some() {
                    hang = Some(lines.len() + 2);
                }
            }
            lines.extend(Self::boxed(
                ui,
                Panel::label(ui, focused(field), name),
                narrow,
                value,
                "",
                border(field),
            ));
        }

        lines.push(Panel::section(ui, "WHAT"));
        let names: Vec<&str> = CAPABILITIES
            .iter()
            .map(|capability| capability.name)
            .collect();
        let mut spans = Panel::label(ui, focused(Field::Capability), "Capability");
        spans.extend(Panel::segmented(
            ui,
            &names,
            grant.capability,
            focused(Field::Capability),
        ));
        spans.push(Span::styled(
            format!(" {}", CAPABILITIES[grant.capability].about),
            ui.fg(Role::Muted),
        ));
        if focused(Field::Capability) {
            focus_line = lines.len();
        }
        lines.push(Line::from(spans));
        let tools = CAPABILITIES[grant.capability].tools;
        let at = self.tool.min(tools.len().saturating_sub(1));
        if focused(Field::Tools) {
            focus_line = lines.len();
        }
        for (row, chunk) in tools.chunks(2).enumerate() {
            let mut spans = if row == 0 {
                let mut spans = Panel::label(ui, focused(Field::Tools), "Tools");
                spans.push(Span::raw(" "));
                spans
            } else {
                vec![Span::raw(" ".repeat(MIXED_LABEL + 1))]
            };
            for (column, tool) in chunk.iter().enumerate() {
                let index = row * 2 + column;
                let on = grant.tools.get(index).copied().unwrap_or(false);
                let picked = focused(Field::Tools) && index == at;
                let style = if picked {
                    ui.on(Role::OnSelection, Role::Selection)
                } else {
                    ui.fg(Role::Foreground)
                };
                spans.push(Span::styled("[", style));
                spans.push(Span::styled(
                    if on { "x" } else { " " },
                    if picked { style } else { ui.bold(Role::Focus) },
                ));
                spans.push(Span::styled(format!("] {tool:<22}"), style));
                spans.push(Span::raw(" "));
            }
            lines.push(Line::from(spans));
        }
        // A typed field: its box; focused, its middle line and where its text ends.
        let typed = |lines: &mut Vec<Line>,
                     field: Field,
                     label: &str,
                     text: &str,
                     after: &str,
                     foot: &str|
         -> Option<(usize, usize)> {
            let at =
                focused(field).then(|| (lines.len() + 1, MIXED_LABEL + 2 + text.chars().count()));
            lines.extend(Self::boxed(
                ui,
                Panel::label(ui, focused(field), label),
                width,
                vec![
                    say(text),
                    Span::styled(after.to_string(), ui.fg(Role::Muted)),
                ],
                foot,
                border(field),
            ));
            at
        };
        if let Some((line, column)) = typed(
            &mut lines,
            Field::Objects,
            "Objects",
            &grant.objects,
            "",
            "db.schema.table · a lone * for any part",
        ) {
            focus_line = line;
            cursor = Some((column, line));
        }

        lines.push(Panel::section(ui, "HOW LONG"));
        let names: Vec<&str> = EXPIRES.iter().map(|(short, _)| *short).collect();
        let mut spans = Panel::label(ui, focused(Field::Expires), "Expires");
        spans.extend(Panel::segmented(
            ui,
            &names,
            grant.expires,
            focused(Field::Expires),
        ));
        if focused(Field::Expires) {
            focus_line = lines.len();
        }
        lines.push(Line::from(spans));
        let mut spans = Panel::label(ui, focused(Field::Ask), "Ask each write");
        spans.push(Span::raw(" "));
        let (mark, word) = if grant.ask {
            ("[x]", " On ")
        } else {
            ("[ ]", " Off")
        };
        spans.push(Span::styled(
            format!("{mark}{word}"),
            if focused(Field::Ask) {
                ui.on(Role::OnSelection, Role::Selection)
            } else {
                ui.fg(Role::Foreground)
            },
        ));
        spans.push(Span::styled(
            format!("  {}", grant.how()),
            ui.fg(Role::Muted),
        ));
        if focused(Field::Ask) {
            focus_line = lines.len();
        }
        lines.push(Line::from(spans));
        if grant.ask
            && let Some((line, column)) = typed(
                &mut lines,
                Field::AskSecs,
                "Wait up to",
                &grant.ask_secs,
                " s",
                "1 to 3600 seconds",
            )
        {
            focus_line = line;
            cursor = Some((column, line));
        }

        // What `dexo mcp grant create --confirm` asks: the connection's name typed again,
        // so a write grant is not given by accident. Said as an instruction, the box under
        // it.
        lines.push(Panel::section(ui, "CONFIRM"));
        let name = grant.connection().name;
        let on = focused(Field::Confirm);
        lines.push(Line::from(vec![
            Span::styled(if on { " › " } else { "   " }, ui.bold(Role::Focus)),
            Span::styled(
                "Type '",
                if on {
                    ui.bold(Role::Focus)
                } else {
                    ui.fg(Role::Foreground)
                },
            ),
            Span::styled(name, ui.bold(Role::Foreground)),
            Span::styled(
                "' to confirm",
                if on {
                    ui.bold(Role::Focus)
                } else {
                    ui.fg(Role::Foreground)
                },
            ),
        ]));
        if on {
            focus_line = lines.len() + 1;
            cursor = Some((5 + grant.confirm.chars().count(), lines.len() + 1));
        }
        lines.extend(Self::boxed(
            ui,
            vec![Span::raw("   ")],
            narrow,
            vec![say(&grant.confirm)],
            "",
            border(Field::Confirm),
        ));
        lines.push(match &self.error {
            Some((_, message)) => Line::styled(format!(" × {message}"), ui.bold(Role::Error)),
            None => Line::raw(""),
        });
        lines.push(Panel::section(ui, "RESULT"));
        lines.push(Line::styled(
            format!("   {}", grant.allows()),
            ui.fg(Role::Foreground),
        ));
        lines.push(Line::styled(
            format!("   {}", grant.lasts()),
            ui.fg(Role::Muted),
        ));
        let production = grant.connection().env == Env::Production;
        if production {
            lines.push(Line::styled(
                "   ! writes to a production connection",
                ui.bold(Role::Warning),
            ));
        }

        let area = Rect {
            height: (lines.len() as u16 + 5).min(area.height),
            ..area
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(ui.fg(Role::Focus))
            .title(Span::styled(" NEW GRANT ", ui.bold(Role::Focus)))
            .style(ui.base());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [body, _, buttons, hints] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        // On a short screen the form scrolls to keep the focused row and its box in view.
        let height = usize::from(body.height);
        let offset = (focus_line + 2).saturating_sub(height);
        frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), body);
        if let Some((column, line)) = cursor
            && line >= offset
        {
            cursor_at(
                frame,
                body,
                body.x + column as u16,
                body.y + (line - offset) as u16,
            );
        }

        let tone = if production {
            Role::Warning
        } else {
            Role::Focus
        };
        let row = Line::from(vec![
            ui.button("Cancel", slot == Slot::Cancel, Role::Focus, false),
            Span::raw("  "),
            ui.button("Create grant", slot == Slot::Create, tone, true),
        ])
        .alignment(Alignment::Center);
        frame.render_widget(Paragraph::new(row), buttons);
        frame.render_widget(Paragraph::new(ui.hints(self.hints(slot))), hints);

        // The open dropdown, hanging from its box over what is below.
        if let (Some(highlight), Some(line), Slot::Field(field)) = (self.open, hang, slot)
            && line >= offset
        {
            let options = Boxes::options(field, grant);
            let screen = frame.area();
            let height = (options.len() as u16 + 2).min(screen.height);
            let list = Rect {
                x: body.x + MIXED_LABEL as u16,
                y: (body.y + (line - offset) as u16).min(screen.height.saturating_sub(height)),
                width: narrow as u16,
                height,
            };
            let inside = narrow.saturating_sub(3);
            let rows: Vec<Line> = options
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    let text = format!(" {option:<inside$}");
                    if index == highlight {
                        Line::styled(text, ui.on(Role::OnFocus, Role::Focus))
                    } else {
                        Line::styled(text, ui.fg(Role::Foreground))
                    }
                })
                .collect();
            frame.render_widget(Clear, list);
            frame.render_widget(
                Paragraph::new(rows).block(
                    Block::bordered()
                        .border_style(ui.fg(Role::Focus))
                        .style(ui.base()),
                ),
                list,
            );
        }
    }
}

// The looks, one at a time.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Panel,
    Boxes,
    Steps,
    Mixed,
}

struct App {
    look: Look,
    grant: Grant,
    panel: Panel,
    boxes: Boxes,
    steps: Steps,
    mixed: Mixed,
    notice: Option<(String, Role, Instant)>,
}

impl App {
    fn new() -> Self {
        Self {
            look: Look::Mixed,
            grant: Grant::new(),
            panel: Panel::default(),
            boxes: Boxes::default(),
            steps: Steps::default(),
            mixed: Mixed::default(),
            notice: None,
        }
    }

    fn key(&mut self, key: KeyEvent) {
        let outcome = match self.look {
            Look::Panel => self.panel.key(key, &mut self.grant),
            Look::Boxes => self.boxes.key(key, &mut self.grant),
            Look::Steps => self.steps.key(key, &mut self.grant),
            Look::Mixed => self.mixed.key(key, &mut self.grant),
        };
        match outcome {
            Outcome::Stay => {}
            Outcome::Cancel => {
                self.grant = Grant::new();
                self.panel = Panel::default();
                self.boxes = Boxes::default();
                self.steps = Steps::default();
                self.mixed = Mixed::default();
                self.notice = Some((
                    "Cancelled: nothing was granted, the form starts over".into(),
                    Role::Muted,
                    Instant::now(),
                ));
            }
            Outcome::Create => match self.grant.problem() {
                Some((field, message)) => match self.look {
                    Look::Panel => self.panel.fail(field, message, &self.grant),
                    Look::Boxes => self.boxes.fail(field, message, &self.grant),
                    Look::Steps => self.steps.fail(field, message, &self.grant),
                    Look::Mixed => self.mixed.fail(field, message, &self.grant),
                },
                None => {
                    self.notice = Some((
                        format!(
                            "✓ Would create: {} · {}",
                            self.grant.allows(),
                            self.grant.lasts()
                        ),
                        Role::Success,
                        Instant::now(),
                    ));
                }
            },
        }
    }
}

fn draw(frame: &mut Frame, app: &App, ui: &Ui) {
    let area = frame.area();
    frame.render_widget(Block::new().style(ui.base()), area);
    let [top, about, _, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    let mut spans = vec![
        Span::styled(" New grant", ui.bold(Role::Foreground)),
        Span::styled(" · four looks   ", ui.fg(Role::Muted)),
    ];
    for (look, key, name) in [
        (Look::Panel, "F1", "Panel"),
        (Look::Boxes, "F2", "Boxes"),
        (Look::Steps, "F3", "Steps"),
        (Look::Mixed, "F4", "v4"),
    ] {
        spans.push(if app.look == look {
            Span::styled(
                format!(" {key} {name} "),
                ui.on(Role::OnFocus, Role::Focus)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!(" {key} {name} "), ui.fg(Role::Muted))
        });
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), top);
    frame.render_widget(
        Paragraph::new(Line::styled("Ctrl+Q quit ", ui.fg(Role::Muted)))
            .alignment(Alignment::Right),
        top,
    );
    let source = match app.look {
        Look::Panel => {
            "From lazydb's profile form: a label column, choices side by side, [x] boxes, a button row."
        }
        Look::Boxes => {
            "From sqlit's connection form: each field a box titled by its name, lists left open, keys in the border."
        }
        Look::Steps => {
            "From tabiew's pickers and lazydb's review: one question at a time, a filter on each list, a review to confirm."
        }
        Look::Mixed => {
            "F1's panel with F2's dropdowns and boxed inputs. Enter creates only on the Create button."
        }
    };
    frame.render_widget(
        Paragraph::new(Line::styled(
            format!(" {source}"),
            ui.fg(Role::Muted).add_modifier(Modifier::ITALIC),
        )),
        about,
    );
    let body = centered(body, 96);
    match app.look {
        Look::Panel => app.panel.draw(frame, body, &app.grant, ui),
        Look::Boxes => app.boxes.draw(frame, body, &app.grant, ui),
        Look::Steps => app.steps.draw(frame, body, &app.grant, ui),
        Look::Mixed => app.mixed.draw(frame, body, &app.grant, ui),
    }
    if let Some((text, role, at)) = &app.notice
        && at.elapsed() < Duration::from_secs(5)
    {
        frame.render_widget(
            Paragraph::new(Line::styled(format!(" {text}"), ui.bold(*role))),
            bottom,
        );
    }
}

fn main() -> std::io::Result<()> {
    let ui = Ui {
        theme: theme_for(Mode::Dark, DEFAULT_ACCENT),
        caps: TerminalCapabilities::detect(),
    };
    let mut terminal = ratatui::init();
    let mut app = App::new();
    let result = (|| -> std::io::Result<()> {
        loop {
            terminal.draw(|frame| draw(frame, &app, &ui))?;
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
                KeyCode::F(1) => app.look = Look::Panel,
                KeyCode::F(2) => app.look = Look::Boxes,
                KeyCode::F(3) => app.look = Look::Steps,
                KeyCode::F(4) => app.look = Look::Mixed,
                _ => app.key(key),
            }
        }
    })();
    ratatui::restore();
    result
}
