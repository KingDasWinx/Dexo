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
    /// The request picked, by id: the list is read again every second, and a position
    /// pointed at another request once one before it was decided.
    pub selected: Option<uuid::Uuid>,
    pub deciding: Option<Deciding>,
    /// When the list was read, in Unix seconds, for the time each request has left.
    pub now: i64,
    /// Requests already announced while the screen was closed.
    pub announced: Vec<uuid::Uuid>,
    /// Lines scrolled down from the picked request's first: PgUp/PgDn and the wheel read
    /// a statement taller than the popup.
    pub scroll: u16,
}

/// Agent Activity laid out for a width: the list and the recent calls scroll, while the
/// confirmation and the keys stay at the bottom, where they cannot fall off the popup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuditView {
    pub body: Vec<String>,
    pub footer: Vec<String>,
    /// The picked request's first and last lines in `body`.
    pub picked: Option<(usize, usize)>,
}

/// `text` wrapped to `width`, its first line after `first` and the rest after `rest`.
fn push_wrapped(lines: &mut Vec<String>, first: &str, rest: &str, text: &str, width: usize) {
    let room = width.saturating_sub(first.len()).max(1);
    for (index, part) in crate::model::wrap_display_text(text, room)
        .into_iter()
        .enumerate()
    {
        let prefix = if index == 0 { first } else { rest };
        lines.push(format!("{prefix}{part}"));
    }
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
        let id = self.selected?;
        self.pending.iter().find(|request| request.id == id)
    }

    fn position(&self) -> Option<usize> {
        let id = self.selected?;
        self.pending.iter().position(|request| request.id == id)
    }

    /// Moves the pick up or down the list.
    pub fn select(&mut self, delta: isize) {
        let Some(last) = self.pending.len().checked_sub(1) else {
            return;
        };
        let index = self
            .position()
            .map_or(0, |index| index.saturating_add_signed(delta).min(last));
        self.selected = Some(self.pending[index].id);
        self.scroll = 0;
    }

    /// Takes the list as read again. The pick stays on its request; one decided elsewhere
    /// gives way to the request now in its place. A confirmation whose request is gone
    /// closes, and true says so: left open, it would settle a request no longer shown.
    pub fn load(&mut self, pending: Vec<Approval>, now: i64) -> bool {
        let was = self.position();
        self.now = now;
        self.announced = pending.iter().map(|request| request.id).collect();
        self.pending = pending;
        if self.current().is_none() {
            self.scroll = 0;
            self.selected = self
                .pending
                .get(was.unwrap_or(0).min(self.pending.len().saturating_sub(1)))
                .map(|request| request.id);
        }
        let gone = self
            .deciding
            .as_ref()
            .is_some_and(|deciding| self.pending.iter().all(|request| request.id != deciding.id));
        if gone {
            self.deciding = None;
        }
        gone
    }

    /// Every line, at any width: the view without a viewport.
    pub fn lines(&self) -> Vec<String> {
        let view = self.view(usize::MAX);
        let mut lines = view.body;
        lines.extend(view.footer);
        lines
    }

    /// The screen laid out `width` cells wide. The picked request's statement is shown
    /// whole and wrapped -- it used to stop at six lines and at the popup's edge, so an
    /// `OR TRUE` on the seventh line, or at the end of a long one, was approved unseen.
    pub fn view(&self, width: usize) -> AuditView {
        let mut body = Vec::new();
        let mut picked = None;
        if self.pending.is_empty() {
            body.push("No agent's write is waiting for approval.".into());
        } else {
            body.push(format!("Waiting for you ({})", self.pending.len()));
            for request in &self.pending {
                let is_picked = self.selected == Some(request.id);
                let first = body.len();
                let marker = if is_picked { "> " } else { "  " };
                push_wrapped(&mut body, marker, "  ", &self.summary(request), width);
                if is_picked {
                    for line in request.statement.lines() {
                        push_wrapped(&mut body, "    ", "    ", line, width);
                    }
                    picked = Some((first, body.len() - 1));
                }
            }
        }
        body.push(String::new());
        body.push("Recent activity".into());
        if self.events.is_empty() {
            body.push("  nothing yet".into());
        }
        for event in self.events.iter().take(20) {
            body.push(format!("  {event}"));
        }
        let mut footer = Vec::new();
        if let Some(deciding) = &self.deciding {
            let question = if deciding.approve {
                "Run this write now?"
            } else {
                "Refuse this write?"
            };
            let label = if deciding.approve { "Approve" } else { "Deny" };
            // The question names its own request, the one the answer settles.
            let asked = match self
                .pending
                .iter()
                .find(|request| request.id == deciding.id)
            {
                Some(request) => format!("{question} {}", self.summary(request)),
                None => question.into(),
            };
            push_wrapped(&mut footer, "", "", &asked, width);
            footer.push(footer_line(label, deciding.focus));
        }
        footer.push(
            "a approve  d deny  up/down pick  PgDn scroll  r revoke all grants  esc close".into(),
        );
        AuditView {
            body,
            footer,
            picked,
        }
    }

    fn summary(&self, request: &Approval) -> String {
        format!(
            "{} on {} · {} · {}s left",
            request.tool,
            request.connection,
            request.targets.join(", "),
            request.seconds_left(self.now)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Deciding, McpAuditScreen};
    use crate::widgets::form::FooterFocus;

    fn request(sql: &str) -> dexo_app::mcp::Approval {
        dexo_app::mcp::Approval::pending(
            "assistant",
            "local",
            "data_execute_sql",
            serde_json::json!({ "sql": sql }).as_object().unwrap(),
            vec!["db.public.orders".into()],
            1000,
            120,
        )
    }

    /// With A, B and C waiting and B picked for approval, A being decided elsewhere keeps
    /// the pick and the confirmation on B; B going closes the confirmation instead of
    /// moving it to the request that took its place.
    #[test]
    fn the_pick_and_its_confirmation_follow_their_request() {
        let (a, b, c) = (
            request("UPDATE a SET x = 1"),
            request("UPDATE b SET x = 1"),
            request("UPDATE c SET x = 1"),
        );
        let mut screen = McpAuditScreen::default();
        assert!(!screen.load(vec![a.clone(), b.clone(), c.clone()], 1001));
        assert_eq!(screen.selected, Some(a.id));
        screen.select(1);
        assert_eq!(screen.selected, Some(b.id));
        screen.deciding = Some(Deciding {
            id: b.id,
            approve: true,
            focus: FooterFocus::Cancel,
        });
        assert!(!screen.load(vec![b.clone(), c.clone()], 1002));
        assert_eq!(screen.current().map(|request| request.id), Some(b.id));
        let lines = screen.lines().join("\n");
        assert!(lines.contains("> data_execute_sql"), "{lines}");
        assert!(lines.contains("UPDATE b SET x = 1"), "{lines}");
        assert!(!lines.contains("UPDATE c SET x = 1"), "{lines}");

        assert!(
            screen.load(vec![c.clone()], 1003),
            "the confirmation closes"
        );
        assert!(screen.deciding.is_none());
        assert_eq!(screen.selected, Some(c.id));
    }
}
