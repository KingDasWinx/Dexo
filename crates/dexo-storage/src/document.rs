use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileFingerprint {
    pub mtime: String,
    pub hash: String,
}

pub fn has_external_conflict(stored: &FileFingerprint, disk: &FileFingerprint) -> bool {
    stored.hash != disk.hash && stored.mtime != disk.mtime
}

pub struct DocumentRepository<'a> {
    conn: &'a Connection,
}

impl<'a> DocumentRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn save(
        &self,
        id: &str,
        project_id: Option<&str>,
        title: &str,
        content: &str,
        path: Option<&str>,
        fingerprint: Option<&FileFingerprint>,
        kind: Option<&str>,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO documents (id, project_id, title, content, path, mtime, content_hash, kind, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET
               project_id = excluded.project_id,
               title = excluded.title,
               content = excluded.content,
               path = excluded.path,
               mtime = excluded.mtime,
               content_hash = excluded.content_hash,
               kind = excluded.kind,
               updated_at = excluded.updated_at",
            params![
                id,
                project_id,
                title,
                content,
                path,
                fingerprint.map(|fp| fp.mtime.as_str()),
                fingerprint.map(|fp| fp.hash.as_str()),
                kind
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> anyhow::Result<Option<StoredDocument>> {
        self.conn
            .query_row(
                "SELECT id, project_id, title, content, path, mtime, content_hash, kind
                 FROM documents WHERE id = ?1",
                params![id],
                row_to_document,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_for_project(&self, project_id: &str) -> anyhow::Result<Vec<StoredDocument>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, title, content, path, mtime, content_hash, kind
             FROM documents WHERE project_id = ?1 ORDER BY title",
        )?;
        let rows = stmt.query_map(params![project_id], row_to_document)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete(&self, id: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM documents WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Empties a project's document rows, and the project-less ones an older save path
    /// left behind -- nothing reads those. The table is the snapshot of what is open,
    /// not a library of everything ever typed: a flush clears it and writes back the
    /// tabs that survived, so a closed tab cannot return on the next launch.
    pub fn clear_project(&self, project_id: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM documents WHERE project_id = ?1 OR project_id IS NULL",
            params![project_id],
        )?;
        Ok(())
    }

    pub fn move_to_project(&self, id: &str, project_id: &str) -> anyhow::Result<()> {
        let changed = self.conn.execute(
            "UPDATE documents SET project_id = ?1 WHERE id = ?2",
            params![project_id, id],
        )?;
        if changed == 0 {
            anyhow::bail!("unknown document {id}");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredDocument {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub content: String,
    pub path: Option<String>,
    pub fingerprint: Option<FileFingerprint>,
    /// What the tab is, as the workbench spells it -- `None` for an ordinary editor
    /// tab. A table browser that loses this comes back as an editor tab, and the next
    /// open of that table makes a second document instead of finding this one.
    pub kind: Option<String>,
}

fn row_to_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredDocument> {
    let mtime: Option<String> = row.get(5)?;
    let hash: Option<String> = row.get(6)?;
    Ok(StoredDocument {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        content: row.get(3)?,
        path: row.get(4)?,
        fingerprint: match (mtime, hash) {
            (Some(mtime), Some(hash)) => Some(FileFingerprint { mtime, hash }),
            _ => None,
        },
        kind: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{FileFingerprint, has_external_conflict};

    #[test]
    fn detects_mtime_and_hash_conflict() {
        let stored = FileFingerprint {
            mtime: "1".into(),
            hash: "aaa".into(),
        };
        let disk = FileFingerprint {
            mtime: "2".into(),
            hash: "bbb".into(),
        };
        assert!(has_external_conflict(&stored, &disk));
        assert!(!has_external_conflict(&stored, &stored));
    }

    #[test]
    fn document_crud_round_trip() {
        let db = crate::Database::open_in_memory().unwrap();
        let repo = super::DocumentRepository::new(db.connection());
        let fp = FileFingerprint {
            mtime: "1".into(),
            hash: "abc".into(),
        };
        repo.save("d1", Some("p1"), "scratch", "select 1", None, Some(&fp), None)
            .unwrap();
        assert_eq!(repo.get("d1").unwrap().unwrap().content, "select 1");
        assert_eq!(repo.list_for_project("p1").unwrap().len(), 1);
        repo.delete("d1").unwrap();
        assert!(repo.get("d1").unwrap().is_none());
    }
}
