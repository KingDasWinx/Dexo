#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrantLine {
    pub id: String,
    pub capability: String,
    pub tools: String,
    pub expires_in_secs: i64,
    pub diff: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpProfilesScreen {
    pub open: bool,
    pub name: String,
    pub enabled: bool,
    pub confirm_enable: bool,
    pub confirm_revoke: bool,
    pub scopes: Vec<String>,
    pub tools: Vec<String>,
    pub resources: Vec<String>,
    pub grants: Vec<GrantLine>,
    pub preview: String,
}

impl McpProfilesScreen {
    pub fn fixture() -> Self {
        Self {
            open: true,
            name: "assistant".into(),
            enabled: false,
            confirm_enable: false,
            confirm_revoke: false,
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
        }
    }

    pub fn confirm_enable(&mut self) {
        self.confirm_enable = true;
        self.enabled = true;
        self.preview = format!(
            "enabled {} scopes={} tools={}",
            self.name,
            self.scopes.len(),
            self.tools.len()
        );
    }

    pub fn tick(&mut self) {
        for grant in &mut self.grants {
            grant.expires_in_secs = grant.expires_in_secs.saturating_sub(1);
        }
    }

    pub fn revoke_all(&mut self) {
        if !self.confirm_revoke {
            self.confirm_revoke = true;
            self.preview = "confirm revoke all grants".into();
            return;
        }
        self.grants.clear();
        self.confirm_revoke = false;
        self.preview = "revoked all grants".into();
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "mcp profile={} enabled={} confirm={}",
            self.name, self.enabled, self.confirm_enable
        )];
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
        screen.confirm_enable();
        assert!(screen.enabled);
        assert!(screen.lines().join("\n").contains("deny db.public.secrets"));
        assert!(screen.lines().join("\n").contains("grant data_write"));
        screen.tick();
        assert_eq!(screen.grants[0].expires_in_secs, 899);
        screen.revoke_all();
        assert!(screen.preview.contains("confirm revoke"));
        screen.revoke_all();
        assert!(screen.grants.is_empty());
        assert!(screen.preview.contains("revoked all"));
    }
}
