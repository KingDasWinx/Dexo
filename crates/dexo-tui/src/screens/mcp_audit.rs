//! Agent Activity: the writes agents are waiting to make under asking grants, to approve
//! or deny, and the latest tool calls from the MCP audit log as they arrive.

use dexo_app::mcp::Approval;

use crate::widgets::form::{FooterFocus, footer_line};

/// An approve or a deny the person has asked for, waiting on its confirmation.
#[derive(Clone, Debug, PartialEq)]
pub struct Deciding {
    pub id: uuid::Uuid,
    pub approve: bool,
    pub focus: FooterFocus,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpAuditScreen {
    pub open: bool,
    /// Newest first, already written as lines.
    pub events: Vec<String>,
    pub confirm_revoke_all: bool,
    /// Writes waiting for a decision, oldest first.
    pub pending: Vec<Approval>,
    pub selected: usize,
    pub deciding: Option<Deciding>,
    /// When the list was read, in Unix seconds, for the time each request has left.
    pub now: i64,
    /// Requests already announced while the screen was closed.
    pub announced: Vec<uuid::Uuid>,
}

impl McpAuditScreen {
    pub fn fixture() -> Self {
        Self {
            open: true,
            events: vec!["assistant tools/call catalog_search allow db.public.items".into()],
            ..Self::default()
        }
    }

    pub fn revoke_all(&mut self) {
        if !self.confirm_revoke_all {
            self.confirm_revoke_all = true;
            return;
        }
        self.confirm_revoke_all = false;
    }

    pub fn current(&self) -> Option<&Approval> {
        self.pending.get(self.selected)
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.pending.is_empty() {
            lines.push("No agent's write is waiting for approval.".into());
        } else {
            lines.push(format!("Waiting for you ({})", self.pending.len()));
            for (index, request) in self.pending.iter().enumerate() {
                let marker = if index == self.selected { ">" } else { " " };
                lines.push(format!(
                    "{marker} {} on {} · {} · {}s left",
                    request.tool,
                    request.connection,
                    request.targets.join(", "),
                    request.seconds_left(self.now)
                ));
                if index == self.selected {
                    for line in request.statement.lines().take(6) {
                        lines.push(format!("    {line}"));
                    }
                }
            }
        }
        if let Some(deciding) = &self.deciding {
            let (question, label) = if deciding.approve {
                ("Run this write now?", "Approve")
            } else {
                ("Refuse this write?", "Deny")
            };
            lines.push(question.into());
            lines.push(footer_line(label, deciding.focus));
        }
        lines.push(String::new());
        lines.push("Recent activity".into());
        if self.events.is_empty() {
            lines.push("  nothing yet".into());
        }
        for event in self.events.iter().take(20) {
            lines.push(format!("  {event}"));
        }
        lines.push(String::new());
        lines.push("a approve  d deny  up/down pick  r revoke all grants  esc close".into());
        lines
    }
}
