use rusqlite::{Connection, params};

/// How a statement's run ended.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HistoryOutcome {
    #[default]
    Ok,
    Failed,
    Cancelled,
}

impl HistoryOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// A row kept before outcomes were is a success: only those were kept.
    fn parse(text: Option<String>) -> Self {
        match text.as_deref() {
            Some("failed") => Self::Failed,
            Some("cancelled") => Self::Cancelled,
            _ => Self::Ok,
        }
    }
}

/// A statement as it was run: on which connection, when (UTC, `YYYY-MM-DD HH:MM:SS`), and
/// how it went.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HistoryRow {
    pub id: String,
    pub sql: String,
    pub connection_id: Option<String>,
    pub created_at: String,
    pub outcome: HistoryOutcome,
    pub duration_ms: Option<u64>,
    /// The rows it returned, or changed.
    pub rows: Option<u64>,
    pub error: Option<String>,
    pub database: Option<String>,
}

/// A run to keep.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NewHistoryEntry {
    pub id: String,
    pub project_id: Option<String>,
    pub connection_id: Option<String>,
    pub sql: String,
    pub outcome: HistoryOutcome,
    pub duration_ms: Option<u64>,
    pub rows: Option<u64>,
    pub error: Option<String>,
    pub database: Option<String>,
}

pub struct HistoryRepository<'a> {
    conn: &'a Connection,
}

impl<'a> HistoryRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn insert(&self, id: &str, connection_id: Option<&str>, sql: &str) -> anyhow::Result<()> {
        self.record(&NewHistoryEntry {
            id: id.to_string(),
            connection_id: connection_id.map(str::to_string),
            sql: sql.to_string(),
            ..NewHistoryEntry::default()
        })
    }

    pub fn record(&self, entry: &NewHistoryEntry) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO sql_history (id, connection_id, sql, created_at, project_id, outcome,
                 duration_ms, row_count, error, database_name)
             VALUES (?1, ?2, ?3, datetime('now'), ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.id,
                entry.connection_id,
                entry.sql,
                entry.project_id,
                entry.outcome.as_str(),
                entry.duration_ms.map(to_sql),
                entry.rows.map(to_sql),
                entry.error,
                entry.database
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, ids: &[String]) -> anyhow::Result<()> {
        let mut stmt = self.conn.prepare("DELETE FROM sql_history WHERE id = ?1")?;
        for id in ids {
            stmt.execute(params![id])?;
        }
        Ok(())
    }

    pub fn prune(&self, max_count: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM sql_history WHERE id NOT IN (
                SELECT id FROM sql_history ORDER BY created_at DESC LIMIT ?1
             )",
            params![max_count],
        )?;
        Ok(())
    }

    pub fn count(&self) -> anyhow::Result<i64> {
        let count = self
            .conn
            .query_row("SELECT COUNT(*) FROM sql_history", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn list(&self, connection_id: Option<&str>) -> anyhow::Result<Vec<(String, String)>> {
        if let Some(connection_id) = connection_id {
            let mut stmt = self.conn.prepare(
                "SELECT id, sql FROM sql_history WHERE connection_id = ?1 ORDER BY created_at DESC",
            )?;
            let rows =
                stmt.query_map(params![connection_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else {
            let mut stmt = self
                .conn
                .prepare("SELECT id, sql FROM sql_history ORDER BY created_at DESC")?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    }

    /// Newest first, with the connection, the time and the outcome of each run; every
    /// connection's with none named.
    pub fn entries(&self, connection_id: Option<&str>) -> anyhow::Result<Vec<HistoryRow>> {
        const COLUMNS: &str = "SELECT id, sql, connection_id, created_at, outcome, duration_ms,
                row_count, error, database_name FROM sql_history";
        let row = |row: &rusqlite::Row<'_>| {
            Ok(HistoryRow {
                id: row.get(0)?,
                sql: row.get(1)?,
                connection_id: row.get(2)?,
                created_at: row.get(3)?,
                outcome: HistoryOutcome::parse(row.get(4)?),
                duration_ms: row.get::<_, Option<i64>>(5)?.map(from_sql),
                rows: row.get::<_, Option<i64>>(6)?.map(from_sql),
                error: row.get(7)?,
                database: row.get(8)?,
            })
        };
        // The rowid breaks a tie within a second: two statements of one run.
        let rows = match connection_id {
            Some(connection_id) => self
                .conn
                .prepare(&format!(
                    "{COLUMNS} WHERE connection_id = ?1 ORDER BY created_at DESC, rowid DESC"
                ))?
                .query_map(params![connection_id], row)?
                .collect::<Result<Vec<_>, _>>()?,
            None => self
                .conn
                .prepare(&format!("{COLUMNS} ORDER BY created_at DESC, rowid DESC"))?
                .query_map([], row)?
                .collect::<Result<Vec<_>, _>>()?,
        };
        Ok(rows)
    }

    pub fn clear_for_connection(&self, connection_id: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM sql_history WHERE connection_id = ?1",
            params![connection_id],
        )?;
        Ok(())
    }

    pub fn clear_all(&self) -> anyhow::Result<()> {
        self.conn.execute("DELETE FROM sql_history", [])?;
        Ok(())
    }

    pub fn list_for_project(&self, project_id: &str) -> anyhow::Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, sql FROM sql_history WHERE project_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![project_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn clear_for_project(&self, project_id: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM sql_history WHERE project_id = ?1",
            params![project_id],
        )?;
        Ok(())
    }
}

/// SQLite's integers are signed.
fn to_sql(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn from_sql(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{HistoryOutcome, HistoryRepository, NewHistoryEntry};
    use crate::Database;

    #[test]
    fn a_failed_run_is_kept_with_its_error_and_old_rows_read_as_ok() {
        let db = Database::open_in_memory().unwrap();
        db.connection()
            .execute(
                "INSERT INTO sql_history (id, connection_id, sql, created_at)
                 VALUES ('old', 'pg', 'select 1', '2026-01-01 00:00:00')",
                [],
            )
            .unwrap();
        let repo = HistoryRepository::new(db.connection());
        repo.record(&NewHistoryEntry {
            id: "new".into(),
            connection_id: Some("pg".into()),
            sql: "select * from nope".into(),
            outcome: HistoryOutcome::Failed,
            duration_ms: Some(2),
            error: Some("relation \"nope\" does not exist".into()),
            database: Some("qa0".into()),
            ..NewHistoryEntry::default()
        })
        .unwrap();
        let rows = repo.entries(None).unwrap();
        assert_eq!(rows[0].id, "new");
        assert_eq!(rows[0].outcome, HistoryOutcome::Failed);
        assert_eq!(rows[0].duration_ms, Some(2));
        assert_eq!(
            rows[0].error.as_deref(),
            Some("relation \"nope\" does not exist")
        );
        assert_eq!(rows[0].database.as_deref(), Some("qa0"));
        assert_eq!(rows[1].outcome, HistoryOutcome::Ok);
        assert_eq!(rows[1].duration_ms, None);
        repo.delete(&["old".to_string()]).unwrap();
        assert_eq!(repo.entries(None).unwrap().len(), 1);
    }
}
