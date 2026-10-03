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
    /// Who the connection logs in as and to which database: a session of Dexo's own on
    /// the server is told by them.
    pub user: Option<String>,
    pub database: Option<String>,
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
    /// Narrows the sessions to those whose user, database or query holds it.
    pub search: crate::screen::widgets::Search,
    /// Idle sessions are listed too; they are hidden otherwise, as pg_activity hides them.
    pub show_idle: bool,
    pub sort: SessionSort,
    /// `k`: a cancel waiting on its confirmation.
    pub cancel: Option<CancelPrompt>,
}

/// The column the sessions are sorted by: the longest running first, else in order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SessionSort {
    #[default]
    Time,
    Pid,
    User,
    Database,
    State,
}

impl SessionSort {
    pub fn next(self) -> Self {
        match self {
            Self::Time => Self::Pid,
            Self::Pid => Self::User,
            Self::User => Self::Database,
            Self::Database => Self::State,
            Self::State => Self::Time,
        }
    }
}

/// Asked before a session's query is cancelled: which one, on which connection.
#[derive(Clone, Debug, PartialEq)]
pub struct CancelPrompt {
    pub session: SessionInfo,
    pub connection: String,
    pub focus: FooterFocus,
}

impl CancelPrompt {
    pub fn lines(&self) -> Vec<String> {
        vec![
            format!(
                "Cancel the query session {} is running? The session stays.",
                self.session.id
            ),
            footer_line("Confirm", self.focus),
        ]
    }
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
                user: Some("dexo".into()),
                database: Some("dexo".into()),
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
                    application: Some("psql".into()),
                    client: Some("127.0.0.1".into()),
                },
                SessionInfo {
                    id: "11".into(),
                    user: Some("dexo".into()),
                    database: Some("dexo".into()),
                    state: "idle in transaction".into(),
                    duration_ms: Some(4000),
                    current_query: Some("lock table items".into()),
                    application: None,
                    client: None,
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

    /// The sessions shown: idle ones only when asked for, those the search finds, sorted.
    pub fn visible(&self) -> Vec<&SessionInfo> {
        let mut shown: Vec<&SessionInfo> = self
            .sessions
            .iter()
            .filter(|session| self.show_idle || !session.state.eq_ignore_ascii_case("idle"))
            .filter(|session| {
                self.search.matches([
                    session.id.as_str(),
                    session.user.as_deref().unwrap_or_default(),
                    session.database.as_deref().unwrap_or_default(),
                    session.state.as_str(),
                    session.current_query.as_deref().unwrap_or_default(),
                    session.application.as_deref().unwrap_or_default(),
                    session.client.as_deref().unwrap_or_default(),
                ])
            })
            .collect();
        let pid = |session: &SessionInfo| session.id.parse::<u64>().unwrap_or(u64::MAX);
        match self.sort {
            SessionSort::Time => shown.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms)),
            SessionSort::Pid => shown.sort_by_key(|session| (pid(session), session.id.clone())),
            SessionSort::User => shown.sort_by(|a, b| a.user.cmp(&b.user)),
            SessionSort::Database => shown.sort_by(|a, b| a.database.cmp(&b.database)),
            SessionSort::State => shown.sort_by(|a, b| a.state.cmp(&b.state)),
        }
        shown
    }

    /// Whether a filter is on: the search, or idle sessions shown.
    pub fn filtered(&self) -> bool {
        !self.search.input.is_empty() || self.show_idle
    }

    pub fn picked(&self) -> Option<&SessionInfo> {
        self.visible().get(self.selected).copied()
    }

    /// Whether `session` is one of Dexo's own on this server: named `dexo`, logged in as
    /// the connection is, on its database. Cancelling it would stop what Dexo is doing,
    /// and ending it would drop the connection.
    pub fn is_you(&self, session: &SessionInfo) -> bool {
        let Some(server) = &self.server else {
            return false;
        };
        session.application.as_deref() == Some("dexo")
            && session.user.is_some()
            && session.user == server.user
            && (server.database.is_none() || session.database == server.database)
    }

    pub fn move_selection(&mut self, down: bool) {
        self.move_by(if down { 1 } else { -1 });
    }

    pub fn move_by(&mut self, delta: isize) {
        let last = self.visible().len().saturating_sub(1);
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

    /// The pick back at the top once what is shown changed under it.
    pub fn reset_pick(&mut self) {
        self.selected = 0;
        self.detail_scroll = 0;
    }

    /// `blocks 764` or `blocked by 419` for a session in the way of another.
    pub fn blocking_note(&self, id: &str) -> Option<String> {
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

    /// The edges about `id`, in words: who waits, for what, and what frees it.
    pub fn blocking_of(&self, id: &str) -> Vec<String> {
        self.blocking_lines()
            .into_iter()
            .filter(|line| {
                line.contains(&format!("Session {id} ")) || line.contains(&format!("blocks {id}:"))
            })
            .collect()
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

    /// How the list is kept fresh, for its title.
    pub fn freshness(&self) -> &'static str {
        if self.paused {
            "paused"
        } else if self.loading && self.sessions.is_empty() {
            "reading"
        } else {
            "every 2s"
        }
    }
}

pub fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `1.2s`, `4m03s`, `2h05m`.
pub fn duration(ms: u64) -> String {
    let secs = ms / 1000;
    match secs {
        0..60 => format!("{}.{}s", secs, (ms % 1000) / 100),
        60..3600 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use dexo_driver_api::SessionInfo;

    use super::{AdminScreen, SessionSort};

    fn session(id: usize, state: &str, ms: u64) -> SessionInfo {
        SessionInfo {
            id: id.to_string(),
            user: Some("dexo".into()),
            database: Some("shop".into()),
            state: state.into(),
            duration_ms: Some(ms),
            current_query: Some(format!("select {id}")),
            application: None,
            client: None,
        }
    }

    /// The pick moves within the list, whatever the key, and leaves the detail at its top.
    #[test]
    fn the_pick_stays_on_the_list() {
        let mut admin = AdminScreen {
            sessions: (1..=17).map(|id| session(id, "active", 1000)).collect(),
            ..AdminScreen::default()
        };
        admin.move_by(100);
        assert_eq!(admin.selected, 16);
        admin.detail_scroll = 3;
        admin.select_first();
        assert_eq!((admin.selected, admin.detail_scroll), (0, 0));
    }

    #[test]
    fn idle_is_hidden_and_the_longest_running_comes_first() {
        let mut admin = AdminScreen {
            sessions: vec![
                session(1, "idle", 9000),
                session(2, "active", 100),
                session(3, "idle in transaction", 5000),
            ],
            ..AdminScreen::default()
        };
        let ids = |admin: &AdminScreen| -> Vec<String> {
            admin
                .visible()
                .iter()
                .map(|session| session.id.clone())
                .collect()
        };
        assert_eq!(ids(&admin), ["3", "2"]);
        admin.show_idle = true;
        assert_eq!(ids(&admin), ["1", "3", "2"]);
        admin.sort = SessionSort::Pid;
        assert_eq!(ids(&admin), ["1", "2", "3"]);
    }

    #[test]
    fn a_blocking_edge_names_the_waiter_and_what_frees_it() {
        let admin = AdminScreen::fixture();
        let text = admin.blocking_of("10").join("\n");
        assert!(text.contains("Session 11 blocks 10"), "{text}");
        assert!(text.contains("a lock on public.items"), "{text}");
        assert!(text.contains("Ending 11 frees it"), "{text}");
        assert_eq!(admin.blocking_note("10").as_deref(), Some("blocked by 11"));
        assert_eq!(admin.blocking_note("11").as_deref(), Some("blocks 10"));
    }
}
