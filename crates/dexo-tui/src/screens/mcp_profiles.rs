#[derive(Clone, Debug, PartialEq)]
pub struct McpProfileSummary {
    pub name: String,
    pub enabled: bool,
    pub scopes: Vec<String>,
    pub tools: Vec<String>,
    /// Live grants for this profile. The screen shows the selected profile's, which
    /// is what makes `r` (revoke the selected profile) mean something on screen.
    pub grants: Vec<GrantLine>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrantLine {
    pub id: String,
    pub capability: String,
    pub tools: String,
    pub expires_in_secs: i64,
    pub diff: String,
    /// Above zero, each write the grant covers waits this long for a person.
    pub ask_secs: u32,
}

use crate::screens::schema_editor::FormField;
use crate::widgets::form::{FooterFocus, footer_line};

/// The grant form's rows, in the order they are walked.
pub const GRANT_CONNECTION: usize = 0;
pub const GRANT_CAPABILITY: usize = 1;
pub const GRANT_TOOLS: usize = 2;
pub const GRANT_SELECTOR: usize = 3;
pub const GRANT_EXPIRES: usize = 4;
pub const GRANT_ASK: usize = 5;
pub const GRANT_ASK_SECS: usize = 6;
pub const GRANT_CONFIRM: usize = 7;

/// New MCP Grant, for the profile picked: what `dexo mcp grant create` asks, "ask before
/// each write" included, so the TUI can make the grant Agent Activity then decides on.
#[derive(Clone, Debug, PartialEq)]
pub struct GrantForm {
    pub fields: Vec<FormField>,
    /// A field, then Create, then Cancel.
    pub focus: usize,
    pub ask: bool,
    pub error: Option<String>,
}

impl GrantForm {
    pub fn new(connection: &str) -> Self {
        let field = |label: &str, value: &str| FormField {
            label: label.into(),
            value: value.into(),
            secret: false,
        };
        Self {
            fields: vec![
                field("connection", connection),
                field("capability", "data_write"),
                field("tools", ""),
                field("selector", ""),
                field("expires", "15m"),
                field("ask before each write", ""),
                field("approval timeout (s)", "120"),
                field("confirm", ""),
            ],
            focus: GRANT_TOOLS,
            ask: false,
            error: None,
        }
    }

    fn slots(&self) -> usize {
        self.fields.len() + 2
    }

    pub fn focus_next(&mut self) {
        self.focus = (self.focus + 1) % self.slots();
    }

    pub fn focus_prev(&mut self) {
        self.focus = (self.focus + self.slots() - 1) % self.slots();
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

    /// A key for the focused row: an edit for a field, or on the ask row Space flips
    /// it and `y`/`n` set it.
    pub fn edit(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        if self.focus != GRANT_ASK {
            if let Some(field) = self.fields.get_mut(self.focus) {
                field.value.handle_key(key);
            }
            return;
        }
        if !(key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) {
            return;
        }
        match key.code {
            KeyCode::Char(' ') => self.ask = !self.ask,
            KeyCode::Char('y' | 'Y') => self.ask = true,
            KeyCode::Char('n' | 'N') => self.ask = false,
            _ => {}
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
            return Err("name the tools the grant allows".into());
        }
        let ask_secs = if self.ask {
            match value(GRANT_ASK_SECS).parse::<u32>() {
                Ok(secs) if (1..=dexo_app::mcp::approval::MAX_TIMEOUT_SECS).contains(&secs) => {
                    Some(secs)
                }
                _ => return Err("the approval timeout is 1 to 3600 seconds".into()),
            }
        } else {
            None
        };
        Ok(dexo_app::mcp::GrantRequest {
            connection: value(GRANT_CONNECTION),
            capability: value(GRANT_CAPABILITY),
            tools,
            selector: value(GRANT_SELECTOR),
            expires: value(GRANT_EXPIRES),
            confirm_target: value(GRANT_CONFIRM),
            ask_secs,
        })
    }

    /// The form's lines; the first field is on the second line.
    pub fn lines(&self, profile: &str) -> Vec<String> {
        let mut lines = vec![format!("For MCP profile {profile}")];
        for (index, field) in self.fields.iter().enumerate() {
            let marker = if index == self.focus { ">" } else { " " };
            let value = match index {
                GRANT_ASK if self.ask => "[x] each write waits for you in Agent Activity",
                GRANT_ASK => "[ ] one write, then the grant is spent",
                _ => field.value.as_str(),
            };
            lines.push(format!("{marker} {}: {value}", field.label));
        }
        lines.push(
            "  tools: data_insert data_update data_delete data_execute_sql · schema_apply_ddl"
                .into(),
        );
        lines.push("  confirm: type the connection or the selector again".into());
        if let Some(error) = &self.error {
            lines.push(format!("  {error}"));
        }
        lines.push(footer_line("Create", self.footer_focus()));
        lines
    }
}

/// What a pending revoke confirmation applies to. One shared bool let a confirmation
/// armed for a single profile commit the global sweep instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RevokeScope {
    Profile(String),
    All,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpProfilesScreen {
    pub open: bool,
    pub name: String,
    pub enabled: bool,
    pub confirm_enable: bool,
    pub confirm_revoke: Option<RevokeScope>,
    pub scopes: Vec<String>,
    pub tools: Vec<String>,
    pub resources: Vec<String>,
    pub grants: Vec<GrantLine>,
    pub preview: String,
    pub profiles: Vec<McpProfileSummary>,
    pub selected: usize,
    /// `g`: a new grant for the selected profile, being filled in.
    pub grant_form: Option<GrantForm>,
}

impl McpProfilesScreen {
    pub fn fixture() -> Self {
        Self {
            open: true,
            name: "assistant".into(),
            enabled: false,
            confirm_enable: false,
            confirm_revoke: None,
            scopes: vec!["allow db.public.*".into(), "deny db.public.secrets".into()],
            tools: vec!["catalog_search".into(), "object_describe".into()],
            resources: vec!["db.public.items".into()],
            grants: vec![GrantLine {
                id: "g1".into(),
                capability: "data_write".into(),
                tools: "data_insert".into(),
                expires_in_secs: 900,
                diff: "profile db.public.* -> grant db.public.items".into(),
                ask_secs: 0,
            }],
            preview: "enable requires local confirmation".into(),
            ..Self::default()
        }
    }

    /// Flips the selected profile, asymmetrically on purpose: enabling hands an MCP
    /// client tool access to the database and arms before it commits, while disabling
    /// only takes access away and should not make you ask twice. Returns the new state
    /// once it actually changed.
    pub fn toggle_selected(&mut self) -> Option<bool> {
        if self.name.is_empty() {
            self.preview = "no MCP profile selected".into();
            return None;
        }
        if self.enabled {
            self.enabled = false;
            self.confirm_enable = false;
            self.preview = format!("disabled {}", self.name);
            return Some(false);
        }
        if !self.confirm_enable {
            self.confirm_enable = true;
            self.preview = format!("confirm enable {}", self.name);
            return None;
        }
        self.confirm_enable = false;
        self.enabled = true;
        self.preview = format!(
            "enabled {} scopes={} tools={}",
            self.name,
            self.scopes.len(),
            self.tools.len()
        );
        Some(true)
    }

    /// Same two-step shape as [`Self::revoke_all`], scoped to the selected profile.
    /// Returns its name once the confirmation is spent.
    pub fn revoke_profile(&mut self) -> Option<String> {
        if self.name.is_empty() {
            self.preview = "no MCP profile selected".into();
            return None;
        }
        let armed = RevokeScope::Profile(self.name.clone());
        if self.confirm_revoke.as_ref() != Some(&armed) {
            self.preview = format!("confirm revoke grants for {}", self.name);
            self.confirm_revoke = Some(armed);
            return None;
        }
        self.confirm_revoke = None;
        Some(self.name.clone())
    }

    pub fn tick(&mut self) {
        for grant in &mut self.grants {
            grant.expires_in_secs = grant.expires_in_secs.saturating_sub(1);
        }
    }

    /// Arms on the first call and commits on the second. Returns true once the
    /// confirmation is spent, so the caller knows to emit the effect.
    pub fn revoke_all(&mut self) -> bool {
        if self.confirm_revoke != Some(RevokeScope::All) {
            self.confirm_revoke = Some(RevokeScope::All);
            self.preview = "confirm revoke all grants".into();
            return false;
        }
        self.grants.clear();
        self.confirm_revoke = None;
        self.preview = "revoked all grants".into();
        true
    }

    /// Commits whatever is armed, for the Enter key and the palette's arm-then-confirm
    /// path. `None` means nothing was armed.
    pub fn confirm_pending_revoke(&mut self) -> Option<RevokeScope> {
        match self.confirm_revoke.clone()? {
            RevokeScope::Profile(_) => self.revoke_profile().map(RevokeScope::Profile),
            RevokeScope::All => self.revoke_all().then_some(RevokeScope::All),
        }
    }

    pub fn load_profiles(&mut self, profiles: Vec<McpProfileSummary>) {
        self.profiles = profiles;
        self.selected = 0;
        self.apply_selected();
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.apply_selected();
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.profiles.len() {
            self.selected += 1;
        }
        self.apply_selected();
    }

    fn apply_selected(&mut self) {
        // A pending confirmation belongs to the profile that armed it.
        self.confirm_enable = false;
        self.confirm_revoke = None;
        match self.profiles.get(self.selected).cloned() {
            Some(profile) => {
                self.name = profile.name;
                self.enabled = profile.enabled;
                self.scopes = profile.scopes;
                self.tools = profile.tools;
                self.grants = profile.grants;
            }
            None => {
                self.name.clear();
                self.enabled = false;
                self.scopes.clear();
                self.tools.clear();
                self.grants.clear();
            }
        }
    }

    pub fn lines(&self) -> Vec<String> {
        if self.profiles.is_empty() && self.name.is_empty() {
            let mut lines = vec!["no MCP profiles".into()];
            if !self.preview.is_empty() {
                lines.push(self.preview.clone());
            }
            return lines;
        }
        let mut lines = self
            .profiles
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let marker = if index == self.selected { ">" } else { " " };
                format!(
                    "{marker} profile {} enabled={}",
                    profile.name, profile.enabled
                )
            })
            .collect::<Vec<_>>();
        lines.push(format!(
            "mcp profile={} enabled={} confirm={}",
            self.name, self.enabled, self.confirm_enable
        ));
        for scope in &self.scopes {
            lines.push(format!("scope {scope}"));
        }
        for tool in &self.tools {
            lines.push(format!("tool {tool}"));
        }
        for resource in &self.resources {
            lines.push(format!("resource {resource}"));
        }
        for grant in &self.grants {
            let asks = if grant.ask_secs > 0 {
                format!(" asks ({}s)", grant.ask_secs)
            } else {
                String::new()
            };
            lines.push(format!(
                "grant {} {} {}s{asks}",
                grant.capability, grant.tools, grant.expires_in_secs
            ));
            lines.push(format!("diff {}", grant.diff));
        }
        if !self.preview.is_empty() {
            lines.push(self.preview.clone());
        }
        lines.push("e enable/disable  g new grant  r revoke  R revoke all  esc close".into());
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::McpProfilesScreen;

    #[test]
    fn sample_starts_disabled_until_confirmed() {
        let mut screen = McpProfilesScreen::fixture();
        assert!(!screen.enabled);
        assert_eq!(screen.toggle_selected(), None, "first press must only arm");
        assert!(!screen.enabled);
        assert!(screen.preview.contains("confirm enable"));
        assert_eq!(screen.toggle_selected(), Some(true), "second press commits");
        assert!(screen.enabled);
        // Disabling is the safe direction, so it commits straight away.
        assert_eq!(screen.toggle_selected(), Some(false));
        assert!(!screen.enabled);
        assert!(screen.lines().join("\n").contains("deny db.public.secrets"));
        assert!(screen.lines().join("\n").contains("grant data_write"));
        screen.tick();
        assert_eq!(screen.grants[0].expires_in_secs, 899);
        assert!(!screen.revoke_all(), "first press arms");
        assert!(screen.preview.contains("confirm revoke"));
        assert!(screen.revoke_all(), "second press commits");
        assert!(screen.grants.is_empty());
        assert!(screen.preview.contains("revoked all"));
    }
}
