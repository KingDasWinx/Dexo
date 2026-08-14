use rusqlite::{Connection, params};

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
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO documents (id, project_id, title, content, path, mtime, content_hash, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               content = excluded.content,
               path = excluded.path,
               mtime = excluded.mtime,
               content_hash = excluded.content_hash,
               updated_at = excluded.updated_at",
            params![
                id,
                project_id,
                title,
                content,
                path,
                fingerprint.map(|fp| fp.mtime.as_str()),
                fingerprint.map(|fp| fp.hash.as_str())
            ],
        )?;
        Ok(())
    }
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
}
