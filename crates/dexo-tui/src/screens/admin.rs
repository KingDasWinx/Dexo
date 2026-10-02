use dexo_driver_api::{BlockingEdge, SessionInfo};

use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

/// The server's sessions, one picked with the arrows; `t` ends the picked one once its
/// id is typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdminScreen {
    pub open: bool,
    pub sessions: Vec<SessionInfo>,
    pub blocking: Vec<BlockingEdge>,
    pub captured_at: String,
    pub selected: usize,
    pub terminate: Option<TerminatePrompt>,
    pub last_error: Option<String>,
}

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

    pub fn lines(&self, width: usize) -> Vec<String> {
        let session = &self.session;
        let mut lines = vec![
            format!(
                "Session {} · {}@{} · {}",
                session.id,
                session.user.as_deref().unwrap_or("-"),
                session.database.as_deref().unwrap_or("-"),
                session.state
            ),
            format!(
                "  {}",
                cut(
                    session.current_query.as_deref().unwrap_or("-"),
                    width.saturating_sub(2)
                )
            ),
            String::new(),
            format!(
                "Type {} to end this session and roll back its work.",
                session.id
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
            open: true,
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
            selected: 0,
            terminate: None,
            last_error: None,
        }
    }

    /// The session the arrows are on.
    pub fn picked(&self) -> Option<&SessionInfo> {
        self.sessions.get(self.selected)
    }

    pub fn move_selection(&mut self, down: bool) {
        let last = self.sessions.len().saturating_sub(1);
        self.selected = if down {
            (self.selected + 1).min(last)
        } else {
            self.selected.saturating_sub(1)
        };
    }

    /// The session list as a table, the picked row marked, then who blocks whom and the
    /// keys. `width` is the popup's inner width.
    pub fn lines(&self, width: usize) -> Vec<String> {
        let mut lines = vec![format!(
            "  {:<8} {:<12} {:<12} {:<20} {:>7}  QUERY",
            "ID", "USER", "DATABASE", "STATE", "TIME"
        )];
        if self.sessions.is_empty() {
            lines.push("  No sessions.".into());
        }
        for (index, session) in self.sessions.iter().enumerate() {
            let row = format!(
                "{} {:<8} {:<12} {:<12} {:<20} {:>7}  {}",
                if index == self.selected { ">" } else { " " },
                cut(&session.id, 8),
                cut(session.user.as_deref().unwrap_or("-"), 12),
                cut(session.database.as_deref().unwrap_or("-"), 12),
                cut(&session.state, 20),
                session
                    .duration_ms
                    .map(duration)
                    .unwrap_or_else(|| "-".into()),
                session
                    .current_query
                    .as_deref()
                    .unwrap_or("-")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            lines.push(cut(&row, width));
        }
        if !self.blocking.is_empty() {
            lines.push(String::new());
            for edge in &self.blocking {
                lines.push(cut(
                    &format!(
                        "  {} blocks {} · {} on {}",
                        edge.blocker,
                        edge.blocked,
                        edge.lock.mode,
                        edge.lock.relation.as_deref().unwrap_or("-")
                    ),
                    width,
                ));
            }
        }
        if let Some(error) = &self.last_error {
            lines.push(String::new());
            lines.push(cut(error, width));
        }
        lines.push(String::new());
        lines.push("up/down pick  t terminate  r refresh  esc close".into());
        lines
    }
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
