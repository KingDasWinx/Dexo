#[derive(Clone, Debug, PartialEq)]
pub struct McpProfileSummary {
    pub name: String,
    pub enabled: bool,
    pub scopes: Vec<String>,
    pub tools: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrantLine {
    pub id: String,
    pub capability: String,
    pub tools: String,
    pub expires_in_secs: i64,
    pub diff: String,
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
            }
            None => {
                self.name.clear();
                self.enabled = false;
                self.scopes.clear();
                self.tools.clear();
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
            lines.push(format!(
                "grant {} {} {}s",
                grant.capability, grant.tools, grant.expires_in_secs
            ));
            lines.push(format!("diff {}", grant.diff));
        }
        if !self.preview.is_empty() {
            lines.push(self.preview.clone());
        }
        lines.push(
            "e enable/disable  r revoke profile  R revoke all  up/down select  esc close".into(),
        );
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
