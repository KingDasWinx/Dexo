//! MCP Profiles: the profiles agents connect through, what each allows, and the grants
//! that let one write for a while. A profile's rules are made with `dexo mcp`; this
//! screen enables, disables and deletes profiles, makes grants and takes them back.

use crate::screens::schema_editor::FormField;
use crate::widgets::form::{FooterFocus, footer_line};

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
        format!(
            "{}{objects} ({}): {how}, ends in {}",
            self.tools.replace(',', ", "),
            self.connection,
            duration_words(self.expires_in_secs)
        )
    }
}

/// The grant form's rows, in the order they are walked.
pub const GRANT_PROFILE: usize = 0;
pub const GRANT_CONNECTION: usize = 1;
pub const GRANT_CAPABILITY: usize = 2;
pub const GRANT_TOOLS: usize = 3;
pub const GRANT_SELECTOR: usize = 4;
pub const GRANT_EXPIRES: usize = 5;
pub const GRANT_ASK: usize = 6;
pub const GRANT_ASK_SECS: usize = 7;
pub const GRANT_CONFIRM: usize = 8;

const CAPABILITIES: [(&str, &str); 3] = [
    (
        "data_write",
        "data_insert data_update data_delete data_execute_sql",
    ),
    ("ddl", "schema_apply_ddl"),
    ("admin", "admin_cancel_query admin_terminate_session"),
];

/// The profiles a grant can be for, with the connections each may use.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileChoice {
    pub name: String,
    pub connections: Vec<String>,
}

/// New MCP Grant: what `dexo mcp grant create` asks, "ask before each write" included, so
/// the TUI can make the grant Agent Activity then decides on.
#[derive(Clone, Debug, PartialEq)]
pub struct GrantForm {
    pub profiles: Vec<ProfileChoice>,
    pub profile: usize,
    /// The text fields, indexed by the row constants; the choices and the checkbox keep
    /// their value elsewhere and leave their slot empty.
    pub fields: Vec<FormField>,
    pub connection: usize,
    pub capability: usize,
    /// A field, then Create, then Cancel.
    pub focus: usize,
    pub ask: bool,
    pub error: Option<String>,
}

impl GrantForm {
    pub fn new(profiles: Vec<ProfileChoice>, profile: usize) -> Self {
        let field = |label: &str, value: &str| FormField {
            label: label.into(),
            value: value.into(),
            secret: false,
        };
        let mut form = Self {
            profiles,
            profile: 0,
            fields: vec![
                field("profile", ""),
                field("connection", ""),
                field("capability", ""),
                field("tools", ""),
                field("selector", ""),
                field("expires", "15m"),
                field("ask before each write", ""),
                field("approval timeout", "120"),
                field("confirm", ""),
            ],
            connection: 0,
            capability: 0,
            focus: 0,
            ask: false,
            error: None,
        };
        form.set_profile(profile);
        form
    }

    fn set_profile(&mut self, index: usize) {
        self.profile = index.min(self.profiles.len().saturating_sub(1));
        self.connection = 0;
    }

    /// The connections the profile may use, or none listed when it may use any.
    fn connections(&self) -> &[String] {
        self.profiles
            .get(self.profile)
            .map_or(&[], |profile| profile.connections.as_slice())
    }

    pub fn profile_name(&self) -> String {
        self.profiles
            .get(self.profile)
            .map(|profile| profile.name.clone())
            .unwrap_or_default()
    }

    fn slots(&self) -> usize {
        self.fields.len() + 2
    }

    /// Whether a row is drawn: the approval timeout means nothing unless the grant asks.
    fn shown(&self, index: usize) -> bool {
        index != GRANT_ASK_SECS || self.ask
    }

    pub fn focus_next(&mut self) {
        loop {
            self.focus = (self.focus + 1) % self.slots();
            if self.focus >= self.fields.len() || self.shown(self.focus) {
                return;
            }
        }
    }

    pub fn focus_prev(&mut self) {
        loop {
            self.focus = (self.focus + self.slots() - 1) % self.slots();
            if self.focus >= self.fields.len() || self.shown(self.focus) {
                return;
            }
        }
    }

    /// Left and Right step between the two buttons once one of them has the focus.
    pub fn toggle_button(&mut self) {
        self.focus = match self.footer_focus() {
            FooterFocus::Submit => self.fields.len() + 1,
            FooterFocus::Cancel => self.fields.len(),
            FooterFocus::Input => self.focus,
        };
    }

    pub fn footer_focus(&self) -> FooterFocus {
        match self.focus.checked_sub(self.fields.len()) {
            None => FooterFocus::Input,
            Some(0) => FooterFocus::Submit,
            Some(_) => FooterFocus::Cancel,
        }
    }

    /// Whether the focused row is picked with Left and Right.
    pub fn on_choice(&self) -> bool {
        self.is_choice(self.focus)
    }

    pub fn is_choice(&self, index: usize) -> bool {
        match index {
            GRANT_PROFILE | GRANT_CAPABILITY => true,
            GRANT_CONNECTION => !self.connections().is_empty(),
            _ => false,
        }
    }

    /// Puts the focus on the field a refusal is about, so the fix is where the cursor is.
    pub fn focus_for_error(&mut self, message: &str) {
        let lower = message.to_lowercase();
        let field = [
            ("capability", GRANT_CAPABILITY),
            ("a time", GRANT_EXPIRES),
            ("lasts from", GRANT_EXPIRES),
            ("tool", GRANT_TOOLS),
            ("confirm", GRANT_CONFIRM),
            ("selector", GRANT_SELECTOR),
            ("profile allows", GRANT_SELECTOR),
            ("narrow the profile", GRANT_SELECTOR),
            ("connection", GRANT_CONNECTION),
            ("approval timeout", GRANT_ASK_SECS),
        ]
        .into_iter()
        .find(|(word, _)| lower.contains(word))
        .map(|(_, field)| field);
        if let Some(field) = field.filter(|field| self.shown(*field)) {
            self.focus = field;
        }
    }

    /// Steps the focused choice.
    pub fn step(&mut self, delta: isize) {
        let wrap = |at: usize, len: usize| (at as isize + delta).rem_euclid(len as isize) as usize;
        self.error = None;
        match self.focus {
            GRANT_PROFILE if !self.profiles.is_empty() => {
                self.set_profile(wrap(self.profile, self.profiles.len()));
            }
            GRANT_CAPABILITY => self.capability = wrap(self.capability, CAPABILITIES.len()),
            GRANT_CONNECTION if !self.connections().is_empty() => {
                self.connection = wrap(self.connection, self.connections().len());
            }
            _ => {}
        }
    }

    /// A key for the focused row: an edit for a field, Left and Right for a choice, and
    /// on the ask row Space flips it and `y`/`n` set it.
    pub fn edit(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        if self.is_choice(self.focus) {
            match key.code {
                KeyCode::Left => self.step(-1),
                KeyCode::Right | KeyCode::Char(' ') => self.step(1),
                _ => {}
            }
            return;
        }
        if self.focus != GRANT_ASK {
            if let Some(field) = self.fields.get_mut(self.focus) {
                field.value.handle_key(key);
            }
            // What was wrong is for the next Create to say again.
            self.error = None;
            return;
        }
        if !(key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) {
            return;
        }
        match key.code {
            KeyCode::Char(' ') => self.toggle_ask(),
            KeyCode::Char('y' | 'Y') => self.set_ask(true),
            KeyCode::Char('n' | 'N') => self.set_ask(false),
            _ => {}
        }
    }

    pub fn toggle_ask(&mut self) {
        self.set_ask(!self.ask);
    }

    fn set_ask(&mut self, ask: bool) {
        self.ask = ask;
        self.error = None;
    }

    fn capability_name(&self) -> &'static str {
        CAPABILITIES[self.capability].0
    }

    fn connection_name(&self) -> String {
        match self.connections().get(self.connection) {
            Some(name) => name.clone(),
            None => self.fields[GRANT_CONNECTION].value.trim().to_string(),
        }
    }

    /// The request the CLI would make with the same answers.
    pub fn request(&self) -> Result<dexo_app::mcp::GrantRequest, String> {
        let value = |index: usize| self.fields[index].value.trim().to_string();
        let tools: Vec<String> = value(GRANT_TOOLS)
            .split([',', ' '])
            .filter(|tool| !tool.is_empty())
            .map(str::to_string)
            .collect();
        if tools.is_empty() {
            return Err(format!(
                "tools: name what the grant allows, from {}",
                CAPABILITIES[self.capability].1
            ));
        }
        if self.profiles.is_empty() {
            return Err(
                "there is no profile to grant to: create one with dexo mcp profile create".into(),
            );
        }
        let ask_secs = if self.ask {
            match value(GRANT_ASK_SECS).parse::<u32>() {
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
        Ok(dexo_app::mcp::GrantRequest {
            connection: self.connection_name(),
            capability: self.capability_name().to_string(),
            tools,
            selector: value(GRANT_SELECTOR),
            expires: value(GRANT_EXPIRES),
            confirm_target: value(GRANT_CONFIRM),
            ask_secs,
        })
    }

    /// The form's lines; the first field is on the second line.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            "Left/Right pick a profile, connection or capability; Space flips the checkbox.".into(),
        ];
        for (index, field) in self.fields.iter().enumerate() {
            if !self.shown(index) {
                continue;
            }
            let marker = if index == self.focus { ">" } else { " " };
            let value = match index {
                GRANT_PROFILE => format!("< {} >", self.profile_name()),
                GRANT_CONNECTION if !self.connections().is_empty() => {
                    format!("< {} >", self.connection_name())
                }
                GRANT_CAPABILITY => format!("< {} >", self.capability_name()),
                GRANT_ASK if self.ask => "[x] each write waits for you in Agent Activity".into(),
                GRANT_ASK => "[ ] one write, then the grant is spent".into(),
                _ => field.value.as_str().to_string(),
            };
            lines.push(format!("{marker} {}: {value}", field.label));
        }
        // Wrapped, not cut: the tools of a capability are a long line.
        for hint in [
            format!(
                "tools for {}: {}",
                self.capability_name(),
                CAPABILITIES[self.capability].1
            ),
            "expires: 90s, 15m, 2h, 1h30m (up to 24h)".to_string(),
            "confirm: type the connection or the selector again".to_string(),
        ] {
            for (index, part) in crate::model::wrap_words(&hint, 86).into_iter().enumerate() {
                lines.push(format!("{}{part}", if index == 0 { "  " } else { "    " }));
            }
        }
        // The row is always there, so the buttons do not move when a message comes.
        let error = self.error.clone().unwrap_or_default();
        let mut shown = crate::model::wrap_words(&error, 86);
        shown.truncate(2);
        shown.resize(2, String::new());
        for part in shown {
            lines.push(if part.is_empty() {
                part
            } else {
                format!("  {part}")
            });
        }
        lines.push(footer_line("Create", self.footer_focus()));
        lines
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
    pub const EMPTY: [&'static str; 6] = [
        "No MCP profiles yet.",
        "Agents connect through a profile, which says what they may see. Make one in a terminal:",
        "  dexo mcp profile create --name assistant",
        "  dexo mcp profile set --name assistant --connection NAME",
        "  dexo mcp allow --profile assistant --selector db.schema.*",
        "Then come back here to enable it and give it writes.",
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
        assert!(text.contains("dexo mcp profile create"), "{text}");
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

    #[test]
    fn the_grant_form_picks_its_profile_and_starts_on_it() {
        let choices = vec![
            ProfileChoice {
                name: "a".into(),
                connections: vec!["pg-dev".into()],
            },
            ProfileChoice {
                name: "b".into(),
                connections: Vec::new(),
            },
        ];
        let mut form = GrantForm::new(choices, 1);
        assert_eq!(form.focus, GRANT_PROFILE);
        assert_eq!(form.profile_name(), "b");
        form.step(-1);
        assert_eq!(form.profile_name(), "a");
        // The connection is the profile's own, not free text to leave empty.
        form.fields[GRANT_TOOLS].value = "data_insert".into();
        form.fields[GRANT_SELECTOR].value = "db.public.t".into();
        let request = form.request().unwrap();
        assert_eq!(request.connection, "pg-dev");
        assert_eq!(request.capability, "data_write");
        let lines = form.lines().join("\n");
        assert!(lines.contains("profile: < a >"), "{lines}");
        assert!(
            !lines.contains("approval timeout"),
            "only when it asks: {lines}"
        );
        form.toggle_ask();
        assert!(form.lines().join("\n").contains("approval timeout"));
    }

    /// A refusal puts the cursor on the field it is about, and the form's size does not
    /// change when it shows, so the buttons stay where they were.
    #[test]
    fn a_refusal_focuses_its_field_and_leaves_the_buttons_in_place() {
        let choices = vec![ProfileChoice {
            name: "a".into(),
            connections: Vec::new(),
        }];
        let mut form = GrantForm::new(choices, 0);
        let before = form.lines().len();
        form.focus_for_error("a grant lasts from 1 second to 24 hours: write the time as 90s");
        assert_eq!(form.focus, GRANT_EXPIRES);
        form.focus_for_error(
            "db.public.* is outside what the profile allows: a grant can only narrow the profile",
        );
        assert_eq!(form.focus, GRANT_SELECTOR);
        form.focus_for_error("confirm: type local or db.public.t exactly as written above");
        assert_eq!(form.focus, GRANT_CONFIRM);
        form.error = Some("a refusal ".repeat(12));
        assert_eq!(form.lines().len(), before, "the buttons do not move");
        form.step(1);
        assert!(form.error.is_none(), "a change takes the old refusal away");
    }
}
