use dexo_driver_api::{AdminPreview, BlockingEdge, SessionInfo};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdminScreen {
    pub open: bool,
    pub sessions: Vec<SessionInfo>,
    pub blocking: Vec<BlockingEdge>,
    pub captured_at: String,
    pub paused: bool,
    pub preview: Option<AdminPreview>,
    pub confirm_target: String,
    pub confirmed: bool,
    pub last_error: Option<String>,
}

impl AdminScreen {
    pub fn fixture() -> Self {
        Self {
            open: true,
            captured_at: "1710000000".into(),
            paused: false,
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
            preview: Some(dexo_driver_api::AdminPreview {
                command: "SELECT pg_terminate_backend(11)".into(),
                lock_risk: dexo_driver_api::LockLevel::None,
                confirmation: dexo_driver_api::AdminConfirmKind::TypeTarget,
            }),
            confirm_target: String::new(),
            confirmed: false,
            last_error: None,
        }
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "admin captured_at={} paused={}",
            self.captured_at, self.paused
        )];
        for session in &self.sessions {
            lines.push(format!(
                "session {} user={} db={} state={} duration_ms={} sql={}",
                session.id,
                session.user.as_deref().unwrap_or("-"),
                session.database.as_deref().unwrap_or("-"),
                session.state,
                session
                    .duration_ms
                    .map(|ms| ms.to_string())
                    .unwrap_or_else(|| "-".into()),
                session.current_query.as_deref().unwrap_or("-")
            ));
        }
        for edge in &self.blocking {
            lines.push(format!(
                "block {} -> {} lock={} rel={}",
                edge.blocker,
                edge.blocked,
                edge.lock.mode,
                edge.lock.relation.as_deref().unwrap_or("-")
            ));
        }
        if let Some(preview) = &self.preview {
            lines.push(format!(
                "preview {} lock={:?} confirm={:?}",
                preview.command, preview.lock_risk, preview.confirmation
            ));
            lines.push(format!(
                "confirm-target={} confirmed={}",
                self.confirm_target, self.confirmed
            ));
        }
        if let Some(error) = &self.last_error {
            lines.push(format!("error={error}"));
        }
        lines
    }
}
