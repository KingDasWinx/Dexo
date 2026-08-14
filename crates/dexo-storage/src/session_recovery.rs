use rusqlite::{Connection, OptionalExtension, params};

use crate::layout::WorkbenchLayout;
use crate::recovery::{RecoveryDocument, RecoveryRepository};

#[derive(Clone, Debug, PartialEq)]
pub struct SessionRecoveryState {
    pub clean_shutdown: bool,
    pub layout: Option<WorkbenchLayout>,
    pub documents: Vec<RecoveryDocument>,
    pub transaction: String,
}

impl SessionRecoveryState {
    pub fn needs_recovery(&self) -> bool {
        !self.clean_shutdown && (!self.documents.is_empty() || self.layout.is_some())
    }
}

pub struct SessionRecoveryRepository<'a> {
    conn: &'a Connection,
}

impl<'a> SessionRecoveryRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn mark_running(&self) -> anyhow::Result<()> {
        self.upsert(false, None, "idle")
    }

    pub fn mark_clean_shutdown(&self) -> anyhow::Result<()> {
        self.upsert(true, None, "idle")
    }

    pub fn checkpoint_layout(
        &self,
        layout: &WorkbenchLayout,
        transaction: &str,
    ) -> anyhow::Result<()> {
        let tx = if transaction == "active" {
            "unknown"
        } else {
            transaction
        };
        // ponytail: crash mid-transaction is unknown, never persisted as active.
        self.upsert(false, Some(layout), tx)
    }

    pub fn load(&self, project_id: &str) -> anyhow::Result<SessionRecoveryState> {
        let row = self
            .conn
            .query_row(
                "SELECT clean_shutdown, layout_json, tx_state FROM session_recovery WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let (clean_shutdown, layout, transaction) = match row {
            Some((flag, json, tx)) => {
                let layout = json
                    .map(|raw| serde_json::from_str(&raw))
                    .transpose()
                    .ok()
                    .flatten();
                (flag != 0, layout, tx)
            }
            None => (true, None, "idle".into()),
        };
        let documents = RecoveryRepository::new(self.conn).list_for_project(project_id)?;
        Ok(SessionRecoveryState {
            clean_shutdown,
            layout,
            documents,
            transaction,
        })
    }

    pub fn discard(&self, project_id: &str) -> anyhow::Result<()> {
        let docs = RecoveryRepository::new(self.conn);
        for doc in docs.list_for_project(project_id)? {
            docs.clear(&doc.id)?;
        }
        self.mark_clean_shutdown()
    }

    fn upsert(
        &self,
        clean: bool,
        layout: Option<&WorkbenchLayout>,
        transaction: &str,
    ) -> anyhow::Result<()> {
        let json = layout.map(serde_json::to_string).transpose()?;
        self.conn.execute(
            "INSERT INTO session_recovery (id, clean_shutdown, layout_json, tx_state, updated_at)
             VALUES (1, ?1, ?2, ?3, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET
               clean_shutdown = excluded.clean_shutdown,
               layout_json = COALESCE(excluded.layout_json, session_recovery.layout_json),
               tx_state = excluded.tx_state,
               updated_at = excluded.updated_at",
            params![if clean { 1 } else { 0 }, json, transaction],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SessionRecoveryRepository;
    use crate::layout::{LayoutRepository, WorkbenchLayout};
    use crate::recovery::RecoveryRepository;
    use crate::{Database, ProjectRepository};
    use dexo_app::{Project, ProjectId};

    fn setup() -> (Database, String) {
        let db = Database::open_in_memory().unwrap();
        let id = uuid::Uuid::new_v4();
        ProjectRepository::new(db.connection())
            .save(&Project {
                id: ProjectId(id),
                name: "p".into(),
                created_at: "now".into(),
            })
            .unwrap();
        (db, id.to_string())
    }

    #[test]
    fn crash_offers_recovery_without_active_transaction() {
        let (db, project) = setup();
        RecoveryRepository::new(db.connection())
            .checkpoint("doc-1", &project, "scratch", "select 1")
            .unwrap();
        SessionRecoveryRepository::new(db.connection())
            .checkpoint_layout(&WorkbenchLayout::default(), "active")
            .unwrap();
        let state = SessionRecoveryRepository::new(db.connection())
            .load(&project)
            .unwrap();
        assert!(state.needs_recovery());
        assert_eq!(state.transaction, "unknown");
        assert_eq!(state.documents.len(), 1);
        assert!(state.layout.is_some());
    }

    #[test]
    fn layout_repo_used_for_checkpoint() {
        let (db, project) = setup();
        LayoutRepository::new(db.connection())
            .save(&project, &WorkbenchLayout::default())
            .unwrap();
        assert!(
            LayoutRepository::new(db.connection())
                .load(&project)
                .unwrap()
                .is_some()
        );
    }
}
