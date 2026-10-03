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
    /// What the screen has to say about the last thing that happened to it, until a key is
    /// pressed: a toast was the only word that a question had lost its request.
    pub notice: Option<String>,
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
    /// The first line of each waiting request in `body`, and which one it is.
    pub rows: Vec<(usize, usize)>,
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

/// What a request would do, for a person: its SQL as it is, a structured write as the
/// fields it sets -- not the call's JSON.
fn readable_statement(request: &Approval) -> String {
    let Ok(serde_json::Value::Object(arguments)) =
        serde_json::from_str::<serde_json::Value>(&request.statement)
    else {
        return request.statement.clone();
    };
    let mut lines = Vec::new();
    let show = |value: &serde_json::Value| match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    for (key, value) in &arguments {
        if let serde_json::Value::Object(fields) = value {
            let inner: Vec<String> = fields
                .iter()
                .map(|(name, field)| format!("{name} = {}", show(field)))
                .collect();
            lines.push(format!("{key}: {}", inner.join(", ")));
        } else if key != "confirm_target" {
            lines.push(format!("{key}: {}", show(value)));
        }
    }
    if request.tool == "admin_terminate_session" {
        lines.insert(0, "ends the server session below:".into());
    }
    lines.join("\n")
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
            let before = self.selected;
            self.selected = self
                .pending
                .get(was.unwrap_or(0).min(self.pending.len().saturating_sub(1)))
                .map(|request| request.id);
            // Only a different pick starts from the top: the list is read again every
            // second, and a reading that left the pick where it was snapped a scrolled
            // list back to its first line.
            if self.selected != before {
                self.scroll = 0;
            }
        }
        let gone = self
            .deciding
            .as_ref()
            .is_some_and(|deciding| self.pending.iter().all(|request| request.id != deciding.id));
        if gone {
            self.deciding = None;
            self.notice = Some(
                "That request was decided elsewhere or ran out of time; nothing was settled."
                    .into(),
            );
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
        let mut rows = Vec::new();
        if self.pending.is_empty() {
            body.push("No agent's write is waiting for approval.".into());
        } else {
            body.push(format!("Waiting for you ({})", self.pending.len()));
            for (index, request) in self.pending.iter().enumerate() {
                let is_picked = self.selected == Some(request.id);
                let first = body.len();
                rows.push((first, index));
                let marker = if is_picked { "> " } else { "  " };
                // Every request says what it would do, the picked one in full below it.
                let mut line = self.summary(request);
                if !is_picked && let Some(first_line) = readable_statement(request).lines().next() {
                    line.push_str(" -- ");
                    line.push_str(&crate::model::truncate_cell(first_line, 48));
                }
                push_wrapped(&mut body, marker, "  ", &line, width);
                if is_picked {
                    for line in readable_statement(request).lines() {
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
        for event in &self.events {
            push_wrapped(&mut body, "  ", "    ", event, width);
        }
        let mut footer = Vec::new();
        if let Some(notice) = &self.notice {
            push_wrapped(&mut footer, "", "", notice, width);
        }
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
            // The statement that would run is in the question, where it is answered.
            if let Some(request) = self
                .pending
                .iter()
                .find(|request| request.id == deciding.id)
            {
                // Its first line, here beside the buttons; the rest is in the list above,
                // which scrolls, so the buttons never leave a short popup.
                let statement = readable_statement(request);
                let mut lines = statement.lines();
                if let Some(first) = lines.next() {
                    let more = lines.count();
                    let tail = if more > 0 {
                        format!("  (+{more} more lines above)")
                    } else {
                        String::new()
                    };
                    footer.push(format!(
                        "    {}{tail}",
                        crate::model::truncate_cell(first, width.saturating_sub(40).max(20))
                    ));
                }
            }
            footer.push(footer_line(label, deciding.focus));
        }
        footer.push(
            "a approve  d deny  up/down pick  PgUp/PgDn scroll  R revoke all grants  esc close"
                .into(),
        );
        AuditView {
            body,
            footer,
            picked,
            rows,
        }
    }

    fn summary(&self, request: &Approval) -> String {
        let gone = if request.seems_gone(self.now) {
            " (the agent has stopped answering)"
        } else {
            ""
        };
        format!(
            "{} on {} · {} · {} left{gone}",
            request.tool,
            request.connection,
            request.targets.join(", "),
            crate::screens::mcp_profiles::duration_words(request.seconds_left(self.now))
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
        // The one not picked says what it is in a line; only the picked one in full.
        assert!(lines.contains("-- UPDATE c SET x = 1"), "{lines}");

        assert!(
            screen.load(vec![c.clone()], 1003),
            "the confirmation closes"
        );
        assert!(screen.deciding.is_none());
        assert_eq!(screen.selected, Some(c.id));
    }

    /// The list is read again every second. With nothing waiting, a reading that left the
    /// pick where it was snapped a scrolled list back to its top.
    #[test]
    fn a_reading_keeps_a_scrolled_list_where_it_was() {
        let mut screen = McpAuditScreen {
            scroll: 7,
            ..McpAuditScreen::default()
        };
        screen.load(Vec::new(), 1001);
        screen.load(Vec::new(), 1002);
        assert_eq!(screen.scroll, 7);
        // A different pick does start from the top.
        let a = request("UPDATE a SET x = 1");
        screen.load(vec![a], 1003);
        assert_eq!(screen.scroll, 0);
    }

    /// Every waiting request says what it would do, not only the picked one, and the
    /// question that settles it repeats the statement.
    #[test]
    fn every_request_says_what_it_does_and_the_question_repeats_it() {
        let (a, b) = (request("UPDATE a SET x = 1"), request("UPDATE b SET x = 1"));
        let mut screen = McpAuditScreen::default();
        screen.load(vec![a.clone(), b.clone()], 1001);
        let lines = screen.lines().join("\n");
        assert!(lines.contains("UPDATE a SET x = 1"), "{lines}");
        assert!(lines.contains("UPDATE b SET x = 1"), "{lines}");
        assert!(
            lines.contains("2 min left"),
            "times are not raw seconds: {lines}"
        );
        screen.deciding = Some(Deciding {
            id: a.id,
            approve: true,
            focus: FooterFocus::Cancel,
        });
        let view = screen.view(100);
        let footer = view.footer.join("\n");
        assert!(footer.contains("Run this write now?"), "{footer}");
        assert!(footer.contains("UPDATE a SET x = 1"), "{footer}");
    }

    /// A structured write is shown as the fields it sets, not as the call's JSON.
    #[test]
    fn a_structured_write_reads_as_its_fields() {
        let arguments = serde_json::json!({
            "target": "public.orders",
            "identity": {"id": 7},
            "values": {"note": "small-term"}
        });
        let request = dexo_app::mcp::Approval::pending(
            "assistant",
            "local",
            "data_update",
            arguments.as_object().unwrap(),
            vec!["db.public.orders".into()],
            1000,
            120,
        );
        let mut screen = McpAuditScreen::default();
        screen.load(vec![request], 1001);
        let lines = screen.lines().join("\n");
        assert!(lines.contains("values: note = small-term"), "{lines}");
        assert!(!lines.contains("{\""), "no raw JSON: {lines}");
    }

    /// A question whose request ran out of time says so on the screen, not only in a toast.
    #[test]
    fn a_question_that_lost_its_request_says_so_on_the_screen() {
        let a = request("UPDATE a SET x = 1");
        let mut screen = McpAuditScreen::default();
        screen.load(vec![a.clone()], 1001);
        screen.deciding = Some(Deciding {
            id: a.id,
            approve: true,
            focus: FooterFocus::Cancel,
        });
        assert!(screen.load(Vec::new(), 1002));
        let lines = screen.lines().join("\n");
        assert!(lines.contains("ran out of time"), "{lines}");
    }
}
