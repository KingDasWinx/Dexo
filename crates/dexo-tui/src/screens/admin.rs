use dexo_driver_api::{BlockingEdge, SessionInfo};

use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

/// The connection the Server screen reads: a session of it, open in Dexo.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerTarget {
    pub session: crate::runtime::SessionId,
    pub generation: u64,
    pub connection: String,
    pub environment: String,
    pub read_only: bool,
}

/// The server's sessions, one picked with the arrows; `t` ends the picked one once its
/// id is typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdminScreen {
    pub server: Option<ServerTarget>,
    pub sessions: Vec<SessionInfo>,
    pub blocking: Vec<BlockingEdge>,
    pub captured_at: String,
    pub selected: usize,
    pub terminate: Option<TerminatePrompt>,
    pub last_error: Option<String>,
    /// A read of the server's sessions is on its way.
    pub loading: bool,
    /// The connection refuses writes: the keys line does not offer `t`.
    pub read_only: bool,
    /// What the last end-session did, where the person looked for it.
    pub notice: Option<String>,
    /// The list is not read again on its own while this is set.
    pub paused: bool,
    /// Seconds since the list was last asked for, toward the next reading.
    pub ticks: u8,
    /// Lines the picked session's details are scrolled down.
    pub detail_scroll: u16,
}

/// How often the Server screen reads the sessions again on its own, in seconds.
pub const REFRESH_SECS: u8 = 2;

/// Asked before a session is ended: its id typed, as `dexo sessions terminate
/// --confirm-target` asks for it.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminatePrompt {
    pub session: SessionInfo,
    /// The connection the session was listed on; the prompt refuses once it changed.
    pub connection: String,
    pub typed: TextInput,
    pub footer: FooterFocus,
    pub error: Option<String>,
}

impl TerminatePrompt {
    pub fn new(session: SessionInfo) -> Self {
        Self {
            session,
            connection: String::new(),
            typed: TextInput::default(),
            footer: FooterFocus::Input,
            error: None,
        }
    }

    pub fn accepted(&self) -> bool {
        self.typed.as_str() == self.session.id
    }

    /// The question, under the session it ends: the pane above already says which.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!(
                "Type {} to end this session and roll back its work.",
                self.session.id
            ),
            self.typed
                .inline_line("id: ", self.footer == FooterFocus::Input),
        ];
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line("Terminate", self.footer));
        lines
    }
}

impl AdminScreen {
    pub fn fixture() -> Self {
        Self {
            server: Some(ServerTarget {
                session: crate::runtime::SessionId(uuid::Uuid::nil()),
                generation: 1,
                connection: "local".into(),
                environment: "local".into(),
                read_only: false,
            }),
            captured_at: "1710000000".into(),
            sessions: vec![
                SessionInfo {
                    id: "10".into(),
                    user: Some("dexo".into()),
                    database: Some("dexo".into()),
                    state: "active".into(),
                    duration_ms: Some(1200),
                    current_query: Some("select 1".into()),
                },
                SessionInfo {
                    id: "11".into(),
                    user: Some("dexo".into()),
                    database: Some("dexo".into()),
                    state: "idle in transaction".into(),
                    duration_ms: Some(4000),
                    current_query: Some("lock table items".into()),
                },
            ],
            blocking: vec![BlockingEdge {
                blocker: "11".into(),
                blocked: "10".into(),
                lock: dexo_driver_api::LockInfo {
                    lock_type: "relation".into(),
                    relation: Some("public.items".into()),
                    mode: "AccessExclusiveLock".into(),
                    granted: false,
                    session_id: "10".into(),
                },
            }],
            ..Self::default()
        }
    }

    pub fn picked(&self) -> Option<&SessionInfo> {
        self.sessions.get(self.selected)
    }

    pub fn move_selection(&mut self, down: bool) {
        self.move_by(if down { 1 } else { -1 });
    }

    pub fn move_by(&mut self, delta: isize) {
        let last = self.sessions.len().saturating_sub(1);
        let moved = self.selected.saturating_add_signed(delta).min(last);
        if moved != self.selected {
            self.detail_scroll = 0;
        }
        self.selected = moved;
    }

    pub fn select_first(&mut self) {
        self.move_by(-(self.sessions.len() as isize));
    }

    pub fn select_last(&mut self) {
        self.move_by(self.sessions.len() as isize);
    }

    /// The column names, `narrow` dropping who and where: the id, state and query
    /// still say which.
    pub fn header(narrow: bool) -> String {
        if narrow {
            format!("{:<7} {:<19} {:>7}  QUERY", "ID", "STATE", "TIME")
        } else {
            format!(
                "{:<8} {:<12} {:<12} {:<19} {:>7}  QUERY",
                "ID", "USER", "DATABASE", "STATE", "TIME"
            )
        }
    }

    /// A session in one row: its query on one line, after who it blocks or waits for.
    pub fn row(&self, session: &SessionInfo, narrow: bool) -> String {
        let time = session
            .duration_ms
            .map(duration)
            .unwrap_or_else(|| "-".into());
        let query = one_line(session.current_query.as_deref().unwrap_or("-"));
        let query = match self.blocking_note(&session.id) {
            Some(note) => format!("[{note}] {query}"),
            None => query,
        };
        if narrow {
            format!(
                "{:<7} {:<19} {:>7}  {query}",
                cut(&session.id, 7),
                cut(&session.state, 19),
                time
            )
        } else {
            format!(
                "{:<8} {:<12} {:<12} {:<19} {:>7}  {query}",
                cut(&session.id, 8),
                cut(session.user.as_deref().unwrap_or("-"), 12),
                cut(session.database.as_deref().unwrap_or("-"), 12),
                cut(&session.state, 19),
                time
            )
        }
    }

    /// The picked session in full, `width` cells wide: who, where, what blocks it or
    /// what it blocks, and its whole query -- the list cuts it at the column's end.
    pub fn detail_lines(&self, width: usize) -> Vec<String> {
        let Some(session) = self.picked() else {
            return Vec::new();
        };
        let mut lines = vec![format!(
            "Session {} · {}@{} · {} · {}",
            session.id,
            session.user.as_deref().unwrap_or("-"),
            session.database.as_deref().unwrap_or("-"),
            session.state,
            session
                .duration_ms
                .map(duration)
                .unwrap_or_else(|| "-".into())
        )];
        for line in self.blocking_lines() {
            if line.contains(&format!("Session {} ", session.id))
                || line.contains(&format!("blocks {}:", session.id))
            {
                lines.push(line);
            }
        }
        lines.push(String::new());
        let query = session.current_query.as_deref().unwrap_or("-");
        for line in query.lines() {
            lines.extend(crate::model::wrap_display_text(line, width.max(8)));
        }
        lines
    }

    /// What each blocking edge says, in words: who waits, for what, and what frees it.
    pub fn blocking_lines(&self) -> Vec<String> {
        self.blocking
            .iter()
            .map(|edge| {
                let lock = &edge.lock;
                let what = match (&lock.relation, lock.lock_type.as_str()) {
                    (Some(table), _) => format!("a lock on {table}"),
                    (None, "transactionid" | "tuple") => "a row lock".to_string(),
                    _ => "a lock".to_string(),
                };
                format!(
                    "Session {} blocks {}: it waits for {what}. Ending {} frees it.",
                    edge.blocker, edge.blocked, edge.blocker
                )
            })
            .collect()
    }

    /// The line over the list: whose sessions, how many, and whether they are kept fresh.
    pub fn summary(&self) -> String {
        let Some(server) = &self.server else {
            return String::new();
        };
        let count = self.sessions.len();
        let blocked = self.blocking.len();
        let mut text = format!(
            "{} · {count} session{}",
            server.connection,
            if count == 1 { "" } else { "s" }
        );
        if blocked > 0 {
            text.push_str(&format!(" · {blocked} blocked"));
        }
        // Only the first reading says so: each one after it would flicker the line.
        text.push_str(if self.paused {
            " · paused"
        } else if self.loading && self.sessions.is_empty() {
            " · reading"
        } else {
            " · every 2s"
        });
        text
    }

    /// The keys, as the status line says them.
    pub fn keys(&self) -> String {
        if self.terminate.is_some() {
            return "type the id  Enter terminate  Esc cancel".into();
        }
        let pause = if self.paused { "p resume" } else { "p pause" };
        if self.read_only {
            format!(
                "Up/Down pick  r refresh  {pause}  c connection  Esc back  (read-only: no terminate)"
            )
        } else {
            format!("Up/Down pick  t terminate  r refresh  {pause}  c connection  Esc back")
        }
    }

    /// `blocks 764` or `blocked by 419` for a session in the way of another.
    fn blocking_note(&self, id: &str) -> Option<String> {
        self.blocking.iter().find_map(|edge| {
            if edge.blocker == id {
                Some(format!("blocks {}", edge.blocked))
            } else if edge.blocked == id {
                Some(format!("blocked by {}", edge.blocker))
            } else {
                None
            }
        })
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `1.2s`, `4m03s`, `2h05m`.
fn duration(ms: u64) -> String {
    let secs = ms / 1000;
    match secs {
        0..60 => format!("{}.{}s", secs, (ms % 1000) / 100),
        60..3600 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

/// `text` in at most `width` characters, an ellipsis marking what was cut.
fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use dexo_driver_api::SessionInfo;

    use super::AdminScreen;

    fn session(id: usize) -> SessionInfo {
        SessionInfo {
            id: id.to_string(),
            user: Some("dexo".into()),
            database: Some("shop".into()),
            state: "idle".into(),
            duration_ms: Some(1000),
            current_query: Some(format!("select {id}")),
        }
    }

    /// The pick moves within the list, whatever the key, and leaves the detail at its top.
    #[test]
    fn the_pick_stays_on_the_list() {
        let mut admin = AdminScreen {
            sessions: (1..=17).map(session).collect(),
            ..AdminScreen::default()
        };
        admin.move_by(100);
        assert_eq!(admin.selected, 16);
        admin.detail_scroll = 3;
        admin.select_first();
        assert_eq!((admin.selected, admin.detail_scroll), (0, 0));
    }

    #[test]
    fn a_blocking_edge_names_the_waiter_and_what_frees_it() {
        let mut admin = AdminScreen::fixture();
        let text = admin.detail_lines(110).join("\n");
        assert!(text.contains("Session 11 blocks 10"), "{text}");
        assert!(text.contains("a lock on public.items"), "{text}");
        assert!(text.contains("Ending 11 frees it"), "{text}");
        assert!(
            admin
                .row(&admin.sessions[0], false)
                .contains("[blocked by 11]")
        );
        assert!(admin.row(&admin.sessions[1], false).contains("[blocks 10]"));
        admin.selected = 1;
        assert!(
            admin
                .detail_lines(110)
                .join("\n")
                .contains("Ending 11 frees it")
        );
    }

    #[test]
    fn a_read_only_connection_does_not_offer_terminate() {
        let mut admin = AdminScreen::fixture();
        assert!(admin.keys().contains("t terminate"));
        admin.read_only = true;
        assert!(!admin.keys().contains("t terminate"));
    }

    /// The query is shown whole in the detail, where the list cuts it at its column.
    #[test]
    fn the_picked_query_is_shown_whole() {
        let mut admin = AdminScreen::fixture();
        let long = format!("select {} from t", "x, ".repeat(60));
        admin.sessions[0].current_query = Some(long.clone());
        let text = admin.detail_lines(40).join("");
        assert!(text.contains("from t"), "{text}");
    }
}
