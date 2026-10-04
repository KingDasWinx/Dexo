use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};

/// Notes people write on a connection's tables and columns, keyed by the connection's id
/// and the object's qualified name as the catalog spells it.
pub struct ObjectNoteRepository<'a> {
    conn: &'a Connection,
}

impl<'a> ObjectNoteRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn get(&self, connection_id: &str, object: &str) -> anyhow::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT note FROM object_notes WHERE connection_id = ?1 AND object = ?2",
                params![connection_id, object],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Writes the note; a blank one removes it.
    pub fn set(&self, connection_id: &str, object: &str, note: &str) -> anyhow::Result<()> {
        if note.trim().is_empty() {
            self.conn.execute(
                "DELETE FROM object_notes WHERE connection_id = ?1 AND object = ?2",
                params![connection_id, object],
            )?;
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO object_notes (connection_id, object, note, updated_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(connection_id, object)
             DO UPDATE SET note = excluded.note, updated_at = excluded.updated_at",
            params![connection_id, object, note.trim()],
        )?;
        Ok(())
    }

    /// Every note of the connection, by object.
    pub fn for_connection(&self, connection_id: &str) -> anyhow::Result<HashMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT object, note FROM object_notes WHERE connection_id = ?1")?;
        let rows = stmt.query_map(params![connection_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<HashMap<_, _>, _>>()
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::ObjectNoteRepository;

    #[test]
    fn notes_stay_with_their_connection_and_a_blank_one_goes() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        for connection in ["a", "b"] {
            conn.execute(
                "INSERT INTO connections (id, name, driver, environment, config_json, secret_ref)
                 VALUES (?1, ?1, 'postgres', 'local', '{}', 'ref')",
                [connection],
            )
            .unwrap();
        }
        let notes = ObjectNoteRepository::new(&conn);
        notes
            .set("a", "db.public.orders", "One row per checkout.")
            .unwrap();
        notes
            .set("b", "db.public.orders", "Another database.")
            .unwrap();
        notes
            .set("a", "db.public.orders", "  One row per paid checkout. ")
            .unwrap();
        assert_eq!(
            notes.get("a", "db.public.orders").unwrap().as_deref(),
            Some("One row per paid checkout.")
        );
        assert_eq!(notes.for_connection("b").unwrap().len(), 1);
        notes.set("a", "db.public.orders", " ").unwrap();
        assert!(notes.get("a", "db.public.orders").unwrap().is_none());
        assert_eq!(
            notes.get("b", "db.public.orders").unwrap().as_deref(),
            Some("Another database.")
        );
        // A note belongs to a saved connection, and goes with it.
        assert!(notes.set("gone", "db.public.orders", "x").is_err());
        conn.execute("DELETE FROM connections WHERE id = 'b'", [])
            .unwrap();
        assert!(notes.for_connection("b").unwrap().is_empty());
    }
}
