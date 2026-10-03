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

/// One call from the MCP audit log, as the Activity view lists it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuditLine {
    /// Local time of day, `HH:MM:SS`.
    pub time: String,
    pub profile: String,
    pub client: String,
    pub tool: String,
    pub target: String,
    /// What came of it, in words: `ok`, `waiting for approval`, `refused: …`.
    pub outcome: String,
    pub duration_ms: u64,
    pub rows: u64,
}

impl AuditLine {
    /// The call in one sentence, as the log reads aloud.
    pub fn sentence(&self) -> String {
        let on = if self.target.is_empty() {
            String::new()
        } else {
            format!(" on {}", self.target)
        };
        format!(
            "{} {}: {}{on} -- {}",
            self.time, self.profile, self.tool, self.outcome
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpAuditScreen {
    /// Newest first.
    pub events: Vec<AuditLine>,
    /// Writes waiting for a decision, oldest first.
    pub pending: Vec<Approval>,
    /// The request picked, by id: the list is read again every second, and a position
    /// pointed at another request once one before it was decided.
    pub selected: Option<uuid::Uuid>,
    pub deciding: Option<Deciding>,
    /// When the list was read, in Unix seconds, for the time each request has left.
    pub now: i64,
    /// Requests already announced while the screen was elsewhere.
    pub announced: Vec<uuid::Uuid>,
    /// What the screen has to say about the last thing that happened to it, until a key is
    /// pressed: a toast was the only word that a question had lost its request.
    pub notice: Option<String>,
    /// Lines scrolled down the picked request's statement.
    pub scroll: u16,
    /// The call picked in the Activity view, among those the filter shows.
    pub event_selected: usize,
    /// Narrows the Activity view to the calls whose sentence holds it.
    pub filter: crate::widgets::text_input::TextInput,
    /// The filter has the keys.
    pub filtering: bool,
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
pub fn readable_statement(request: &Approval) -> String {
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
            events: vec![AuditLine {
                time: "12:00:00".into(),
                profile: "assistant".into(),
                client: "claude-code".into(),
                tool: "catalog_search".into(),
                target: "db.public.items".into(),
                outcome: "ok".into(),
                duration_ms: 12,
                rows: 3,
            }],
            ..Self::default()
        }
    }

    pub fn current(&self) -> Option<&Approval> {
        let id = self.selected?;
        self.pending.iter().find(|request| request.id == id)
    }

    /// The picked request's place in the list.
    pub fn position(&self) -> Option<usize> {
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
            // statement back to its first line.
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

    /// A waiting request in one row of the list, ending with what it would do: only the
    /// picked one said so, and the others had to be picked to be read.
    pub fn row(&self, request: &Approval) -> String {
        let statement = readable_statement(request);
        format!(
            "{}  {}  {}  {}  {}",
            clock(request.created_at),
            request.profile,
            request.tool,
            request.connection,
            statement.lines().next().unwrap_or_default()
        )
    }

    /// The picked request in full, `width` cells wide: what it would run, wrapped and
    /// whole -- a statement cut at a popup's edge was approved unseen.
    pub fn request_lines(&self, width: usize) -> Vec<String> {
        let Some(request) = self.current() else {
            return Vec::new();
        };
        let asked = format!(
            "asked by {} at {}",
            request.profile,
            clock(request.created_at)
        );
        let mut lines: Vec<String> = [self.summary(request), asked]
            .iter()
            .flat_map(|text| crate::model::wrap_words(text, width.max(8)))
            .collect();
        lines.push(String::new());
        for line in readable_statement(request).lines() {
            push_wrapped(&mut lines, "", "  ", line, width);
        }
        lines
    }

    /// The question an approve or a deny waits on, with its buttons, or the keys that ask
    /// it; and what the screen last had to say.
    pub fn decision_lines(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(notice) = &self.notice {
            push_wrapped(&mut lines, "", "", notice, width);
        }
        let Some(deciding) = &self.deciding else {
            if self.current().is_some() {
                lines.push("a approve  d deny".into());
            }
            return lines;
        };
        let question = if deciding.approve {
            "Run this write now?"
        } else {
            "Refuse this write?"
        };
        // The request it settles is the one right above it, in the same pane.
        push_wrapped(&mut lines, "", "", question, width);
        lines.push(footer_line(
            if deciding.approve { "Approve" } else { "Deny" },
            deciding.focus,
        ));
        lines
    }

    /// The calls the filter lets through, newest first.
    pub fn visible_events(&self) -> Vec<&AuditLine> {
        let needle = self.filter.as_str().trim().to_lowercase();
        self.events
            .iter()
            .filter(|event| needle.is_empty() || event.sentence().to_lowercase().contains(&needle))
            .collect()
    }

    /// Moves the Activity view's pick, kept on the calls shown.
    pub fn select_event(&mut self, delta: isize) {
        let last = self.visible_events().len().saturating_sub(1);
        self.event_selected = self.event_selected.saturating_add_signed(delta).min(last);
    }

    pub fn summary(&self, request: &Approval) -> String {
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

/// Unix seconds as the local time of day.
fn clock(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
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
        let lines = screen.request_lines(100).join("\n");
        assert!(lines.contains("data_execute_sql"), "{lines}");
        assert!(lines.contains("UPDATE b SET x = 1"), "{lines}");
        // The one not picked says what it is in its row; only the picked one in full.
        assert!(screen.row(&c).contains("UPDATE c SET x = 1"));

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
    /// question that settles it has its buttons.
    #[test]
    fn every_request_says_what_it_does_and_the_question_repeats_it() {
        let (a, b) = (request("UPDATE a SET x = 1"), request("UPDATE b SET x = 1"));
        let mut screen = McpAuditScreen::default();
        screen.load(vec![a.clone(), b.clone()], 1001);
        assert!(screen.row(&a).contains("UPDATE a SET x = 1"));
        assert!(screen.row(&b).contains("UPDATE b SET x = 1"));
        let lines = screen.request_lines(100).join("\n");
        assert!(
            lines.contains("2 min left"),
            "times are not raw seconds: {lines}"
        );
        screen.deciding = Some(Deciding {
            id: a.id,
            approve: true,
            focus: FooterFocus::Cancel,
        });
        let question = screen.decision_lines(100).join("\n");
        assert!(question.contains("Run this write now?"), "{question}");
        assert!(question.contains("[Approve]"), "{question}");
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
        let lines = screen.request_lines(100).join("\n");
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
        let lines = screen.decision_lines(100).join("\n");
        assert!(lines.contains("ran out of time"), "{lines}");
    }
}
