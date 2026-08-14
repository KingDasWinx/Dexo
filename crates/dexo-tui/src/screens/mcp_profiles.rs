#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpProfilesScreen {
    pub open: bool,
    pub name: String,
    pub enabled: bool,
    pub confirm_enable: bool,
    pub scopes: Vec<String>,
    pub tools: Vec<String>,
    pub preview: String,
}

impl McpProfilesScreen {
    pub fn fixture() -> Self {
        Self {
            open: true,
            name: "assistant".into(),
            enabled: false,
            confirm_enable: false,
            scopes: vec!["allow db.public.*".into(), "deny db.public.secrets".into()],
            tools: vec!["catalog_search".into(), "object_describe".into()],
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
    fn fixture_starts_disabled_until_confirmed() {
        let mut screen = McpProfilesScreen::fixture();
        assert!(!screen.enabled);
        screen.confirm_enable();
        assert!(screen.enabled);
        assert!(screen.lines().join("\n").contains("deny db.public.secrets"));
    }
}
