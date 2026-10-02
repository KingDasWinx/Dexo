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
    /// A read of the server's sessions is on its way; the list shows "Loading" until it
    /// lands, and Esc closes the dialog meanwhile.
    pub loading: bool,
    /// The first session row drawn, once the list is longer than the box.
    pub offset: usize,
    /// The connection refuses writes: the keys line does not offer `t`.
    pub read_only: bool,
    /// What the last end-session did, in the dialog where the person looked for it.
    pub notice: Option<String>,
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
            loading: false,
            offset: 0,
            read_only: false,
            notice: None,
        }
    }

    /// The session the arrows are on.
    pub fn picked(&self) -> Option<&SessionInfo> {
        self.sessions.get(self.selected)
    }

    pub fn move_selection(&mut self, down: bool) {
        self.move_by(if down { 1 } else { -1 });
    }

    /// Moves the pick `delta` rows, stopping at the ends of the list.
    pub fn move_by(&mut self, delta: isize) {
        let last = self.sessions.len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    pub fn select_last(&mut self) {
        self.selected = self.sessions.len().saturating_sub(1);
    }

    const MAX_BLOCKING: usize = 3;

    /// The lines around the list: the header, who blocks whom, a message, the keys.
    fn chrome(&self) -> usize {
        let blocking = self.blocking.len();
        let blocking_rows = if blocking == 0 {
            0
        } else {
            1 + blocking.min(Self::MAX_BLOCKING) + usize::from(blocking > Self::MAX_BLOCKING)
        };
        1 + blocking_rows
            + 2 * usize::from(self.last_error.is_some())
            + usize::from(self.notice.is_some())
            + 2
    }

    /// How many session rows a terminal `height` rows tall leaves room for: the popup
    /// keeps a row of margin above and below, and a border.
    pub fn visible_rows(&self, height: u16) -> usize {
        let room = (height as usize).saturating_sub(4 + self.chrome()).max(3);
        // A list that does not fit says which rows are on show.
        if self.sessions.len() > room {
            room - 1
        } else {
            room
        }
    }

    /// The first row drawn: the window moves only when the pick leaves it.
    pub fn window_start(&self, rows: usize) -> usize {
        crate::palette::scroll_to_selection(self.selected, self.offset, self.sessions.len(), rows)
    }

    /// What each blocking edge says, in words: who waits, for what, and what frees it.
    fn blocking_lines(&self) -> Vec<String> {
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
                    "  Session {} blocks {}: it waits for {what}. Ending {} frees it.",
                    edge.blocker, edge.blocked, edge.blocker
                )
            })
            .collect()
    }

    /// The session list as a table, the picked row marked, then who blocks whom and the
    /// keys. `width` is the popup's inner width, `rows` how many sessions fit.
    pub fn lines(&self, width: usize, rows: usize) -> Vec<String> {
        // A narrow terminal drops who and where: the id, state and query still say which.
        let narrow = width < 84;
        let mut lines = vec![if narrow {
            format!("  {:<7} {:<19} {:>7}  QUERY", "ID", "STATE", "TIME")
        } else {
            format!(
                "  {:<8} {:<12} {:<12} {:<19} {:>7}  QUERY",
                "ID", "USER", "DATABASE", "STATE", "TIME"
            )
        }];
        if self.sessions.is_empty() {
            lines.push(if self.loading {
                "  Loading sessions...".into()
            } else {
                "  No sessions.".into()
            });
        }
        let start = self.window_start(rows);
        for (index, session) in self.sessions.iter().enumerate().skip(start).take(rows) {
            let mark = if index == self.selected { ">" } else { " " };
            let time = session
                .duration_ms
                .map(duration)
                .unwrap_or_else(|| "-".into());
            let query = session
                .current_query
                .as_deref()
                .unwrap_or("-")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let query = match self.blocking_note(&session.id) {
                Some(note) => format!("[{note}] {query}"),
                None => query,
            };
            let row = if narrow {
                format!(
                    "{mark} {:<7} {:<19} {:>7}  {query}",
                    cut(&session.id, 7),
                    cut(&session.state, 19),
                    time
                )
            } else {
                format!(
                    "{mark} {:<8} {:<12} {:<12} {:<19} {:>7}  {query}",
                    cut(&session.id, 8),
                    cut(session.user.as_deref().unwrap_or("-"), 12),
                    cut(session.database.as_deref().unwrap_or("-"), 12),
                    cut(&session.state, 19),
                    time
                )
            };
            lines.push(cut(&row, width));
        }
        if self.sessions.len() > rows {
            lines.push(format!(
                "  Sessions {}-{} of {}",
                start + 1,
                (start + rows).min(self.sessions.len()),
                self.sessions.len()
            ));
        }
        if !self.blocking.is_empty() {
            lines.push(String::new());
            let all = self.blocking_lines();
            for line in all.iter().take(Self::MAX_BLOCKING) {
                lines.push(cut(line, width));
            }
            if all.len() > Self::MAX_BLOCKING {
                lines.push(format!("  and {} more", all.len() - Self::MAX_BLOCKING));
            }
        }
        if let Some(error) = &self.last_error {
            lines.push(String::new());
            lines.push(cut(error, width));
        }
        if let Some(notice) = &self.notice {
            lines.push(cut(notice, width));
        }
        lines.push(String::new());
        lines.push(cut(
            if self.read_only {
                "up/down pick  r refresh  esc close  (read-only: no terminate)"
            } else {
                "up/down pick  t terminate  r refresh  esc close"
            },
            width,
        ));
        lines
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

    fn long_list() -> AdminScreen {
        AdminScreen {
            open: true,
            sessions: (1..=17).map(session).collect(),
            ..AdminScreen::default()
        }
    }

    /// 17 sessions in a 20-row terminal: the list scrolls with the pick, whatever the
    /// key, and the line under it says which rows are on show.
    #[test]
    fn the_pick_is_always_on_a_row_that_is_drawn() {
        let mut admin = long_list();
        let rows = admin.visible_rows(20);
        assert!(rows < 17);
        for _ in 0..16 {
            admin.move_selection(true);
            admin.offset = admin.window_start(rows);
            let shown = admin.lines(58, rows).join("\n");
            let id = &admin.picked().unwrap().id;
            assert!(shown.contains(&format!("> {id:<7}")), "{shown}");
        }
        assert!(admin.lines(58, rows).join("\n").contains("of 17"));
        admin.select_first();
        admin.offset = admin.window_start(rows);
        assert!(admin.lines(58, rows).join("\n").contains("> 1 "));
        admin.move_by(100);
        assert_eq!(admin.selected, 16);
    }

    /// The list's lines never outgrow the terminal they were sized for.
    #[test]
    fn the_box_fits_the_terminal() {
        let mut admin = long_list();
        admin.last_error = Some("Not terminated: x".into());
        admin.notice = Some("Session 4 terminated.".into());
        for height in [14u16, 20, 24, 36] {
            let rows = admin.visible_rows(height);
            let lines = admin.lines(58, rows).len();
            assert!(
                lines + 4 <= height as usize,
                "{lines} lines in {height} rows"
            );
        }
    }

    #[test]
    fn a_blocking_edge_names_the_waiter_and_what_frees_it() {
        let admin = AdminScreen::fixture();
        let text = admin.lines(110, 5).join("\n");
        assert!(text.contains("Session 11 blocks 10"), "{text}");
        assert!(text.contains("a lock on public.items"), "{text}");
        assert!(text.contains("Ending 11 frees it"), "{text}");
        assert!(text.contains("[blocks 10]"), "{text}");
        assert!(text.contains("[blocked by 11]"), "{text}");
    }

    #[test]
    fn a_read_only_connection_does_not_offer_terminate() {
        let mut admin = AdminScreen::fixture();
        assert!(admin.lines(110, 5).join("\n").contains("t terminate"));
        admin.read_only = true;
        assert!(!admin.lines(110, 5).join("\n").contains("t terminate"));
    }
}
