#[derive(Clone, Debug, PartialEq)]
pub struct McpAuditScreen {
    pub open: bool,
    pub events: Vec<String>,
    pub confirm_revoke_all: bool,
}

impl Default for McpAuditScreen {
    fn default() -> Self {
        Self {
            open: false,
            events: Vec::new(),
            confirm_revoke_all: false,
        }
    }
}

impl McpAuditScreen {
    pub fn fixture() -> Self {
        Self {
            open: true,
            events: vec!["allow catalog_search db.public.items".into()],
            confirm_revoke_all: false,
        }
    }

    pub fn revoke_all(&mut self) {
        if !self.confirm_revoke_all {
            self.confirm_revoke_all = true;
            return;
        }
        self.events.clear();
        self.confirm_revoke_all = false;
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("mcp audit confirm_revoke={}", self.confirm_revoke_all),
        ];
        for event in &self.events {
            lines.push(format!("audit {event}"));
        }
        if self.events.is_empty() {
            lines.push("audit empty".into());
        }
        lines
    }
}
