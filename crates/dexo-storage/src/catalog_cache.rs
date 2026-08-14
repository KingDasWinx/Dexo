use dexo_driver_api::{CatalogObject, QualifiedName};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

pub struct CatalogCache<'a> {
    conn: &'a Connection,
}

impl<'a> CatalogCache<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn replace_snapshot(
        &self,
        connection_id: &str,
        database_name: &str,
        objects: &[CatalogObject],
    ) -> anyhow::Result<String> {
        let snapshot_id = Uuid::new_v4().to_string();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO catalog_snapshots(id, connection_id, database_name, complete, created_at)
             VALUES (?1, ?2, ?3, 1, datetime('now'))",
            params![snapshot_id, connection_id, database_name],
        )?;
        for object in objects {
            tx.execute(
                "INSERT INTO catalog_objects(snapshot_id, object_id, parent_id, kind, qualified_name, json, stale)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
                params![
                    snapshot_id,
                    object.id.as_str(),
                    object.parent.as_ref().map(|id| id.as_str().to_string()),
                    object.kind.as_str(),
                    object.qualified_name.display_unquoted(),
                    serde_json::to_string(object)?,
                ],
            )?;
        }
        tx.execute(
            "DELETE FROM catalog_snapshots
             WHERE connection_id = ?1 AND database_name = ?2 AND complete = 1 AND id <> ?3",
            params![connection_id, database_name, snapshot_id],
        )?;
        tx.commit()?;
        Ok(snapshot_id)
    }

    pub fn load_latest(
        &self,
        connection_id: &str,
        database_name: &str,
    ) -> anyhow::Result<Vec<CatalogObject>> {
        let snapshot_id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM catalog_snapshots
                 WHERE connection_id = ?1 AND database_name = ?2 AND complete = 1
                 ORDER BY created_at DESC LIMIT 1",
                params![connection_id, database_name],
                |row| row.get(0),
            )
            .optional()?;
        let Some(snapshot_id) = snapshot_id else {
            return Ok(Vec::new());
        };
        self.load_snapshot(&snapshot_id)
    }

    pub fn load_latest_any(&self) -> anyhow::Result<Vec<CatalogObject>> {
        let snapshot_id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM catalog_snapshots WHERE complete = 1
                 ORDER BY created_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match snapshot_id {
            Some(id) => self.load_snapshot(&id),
            None => Ok(Vec::new()),
        }
    }

    pub fn load_snapshot(&self, snapshot_id: &str) -> anyhow::Result<Vec<CatalogObject>> {
        let mut stmt = self.conn.prepare(
            "SELECT json FROM catalog_objects WHERE snapshot_id = ?1 ORDER BY qualified_name",
        )?;
        let objects = stmt
            .query_map(params![snapshot_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|json| serde_json::from_str(&json).map_err(anyhow::Error::from))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(objects)
    }

    pub fn invalidate(&self, name: &QualifiedName) -> anyhow::Result<()> {
        let qualified = name.display_unquoted();
        let snapshot_id: Option<String> = self
            .conn
            .query_row(
                "SELECT snapshot_id FROM catalog_objects WHERE qualified_name = ?1 LIMIT 1",
                params![qualified],
                |row| row.get(0),
            )
            .optional()?;
        let Some(snapshot_id) = snapshot_id else {
            return Ok(());
        };
        self.conn.execute(
            "UPDATE catalog_objects SET stale = 1
             WHERE snapshot_id = ?1 AND (qualified_name = ?2 OR qualified_name LIKE ?2 || '.%')",
            params![snapshot_id, qualified],
        )?;
        Ok(())
    }

    pub fn is_stale(&self, qualified: &str) -> bool {
        self.conn
            .query_row(
                "SELECT stale FROM catalog_objects WHERE qualified_name = ?1 LIMIT 1",
                params![qualified],
                |row| row.get::<_, i64>(0),
            )
            .ok()
            .is_some_and(|stale| stale != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::CatalogCache;
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    use rusqlite::Connection;

    fn fixture_cache() -> CatalogCache<'static> {
        let conn = Box::leak(Box::new(Connection::open_in_memory().unwrap()));
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::apply_pending(conn).unwrap();
        let objects = [
            CatalogObject::new(
                ObjectId::new("obj:orders"),
                ObjectKind::Table,
                QualifiedName::new(Some("db"), Some("public"), "orders"),
                Some(ObjectId::new("obj:public")),
            ),
            CatalogObject::new(
                ObjectId::new("obj:users"),
                ObjectKind::Table,
                QualifiedName::new(Some("db"), Some("public"), "users"),
                Some(ObjectId::new("obj:public")),
            ),
        ];
        let cache = CatalogCache::new(conn);
        cache.replace_snapshot("c1", "db", &objects).unwrap();
        cache
    }

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        let orders = CatalogObject::new(
            ObjectId::new("obj:orders"),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), "orders"),
            Some(ObjectId::new("obj:public")),
        );
        let users = CatalogObject::new(
            ObjectId::new("obj:users"),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), "users"),
            Some(ObjectId::new("obj:public")),
        );
        let schema = CatalogObject::new(
            ObjectId::new("obj:public"),
            ObjectKind::Schema,
            QualifiedName::new(Some("db"), Some("public"), "public"),
            Some(ObjectId::new("obj:db")),
        );
        let objects = vec![schema, orders, users];
        CatalogCache::new(&conn)
            .replace_snapshot("c1", "db", &objects)
            .unwrap();
        conn
    }

    #[test]
    fn ddl_invalidates_only_affected_subtree() {
        let cache = fixture_cache();
        cache
            .invalidate(&QualifiedName::new(Some("db"), Some("public"), "orders"))
            .unwrap();
        assert!(cache.is_stale("db.public.orders"));
        assert!(!cache.is_stale("db.public.users"));
    }

    #[test]
    fn rolled_back_ddl_keeps_cache_valid_unknown_marks_stale() {
        let conn = fixture();
        let cache = CatalogCache::new(&conn);
        let target = QualifiedName::new(Some("db"), Some("public"), "orders");
        assert!(!cache.is_stale("db.public.orders"));
        let keep = dexo_app::schema::invalidate_after_ddl(
            dexo_driver_api::DdlOutcome::RolledBack,
            &target,
        );
        assert_eq!(keep, dexo_app::schema::CacheAction::Keep);
        assert!(!cache.is_stale("db.public.orders"));
        let uncertain =
            dexo_app::schema::invalidate_after_ddl(dexo_driver_api::DdlOutcome::Unknown, &target);
        assert_eq!(uncertain, dexo_app::schema::CacheAction::MarkUncertain);
        cache.invalidate(&target).unwrap();
        assert!(cache.is_stale("db.public.orders"));
        assert!(!cache.is_stale("db.public.users"));
    }

    #[test]
    fn offline_load_returns_latest_complete_snapshot() {
        let conn = fixture();
        let cache = CatalogCache::new(&conn);
        let loaded = cache.load_latest("c1", "db").unwrap();
        assert_eq!(loaded.len(), 3);
        let replacement = vec![CatalogObject::new(
            ObjectId::new("obj:orders2"),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), "orders"),
            None,
        )];
        cache.replace_snapshot("c1", "db", &replacement).unwrap();
        let loaded = cache.load_latest("c1", "db").unwrap();
        assert_eq!(loaded.len(), 1);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM catalog_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}
