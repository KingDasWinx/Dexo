use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::migrations::{self, LATEST_SCHEMA_VERSION};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub database: PathBuf,
    pub config: PathBuf,
}

impl AppPaths {
    pub fn from_data_home(data_dir: PathBuf) -> Self {
        Self {
            database: data_dir.join("dexo.db"),
            config: data_dir.join("config.toml"),
            data_dir,
        }
    }

    pub fn discover() -> anyhow::Result<Self> {
        if let Ok(dir) = std::env::var("DEXO_DATA_HOME")
            && !dir.is_empty()
        {
            return Ok(Self::from_data_home(PathBuf::from(dir)));
        }
        let dirs = directories::ProjectDirs::from("dev", "dexo", "Dexo")
            .ok_or_else(|| anyhow::anyhow!("platform data directory is unavailable"))?;
        Ok(Self::from_data_home(dirs.data_local_dir().to_path_buf()))
    }
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        prepare_connection(&conn)?;
        migrations::apply_pending(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory_at(version: u32) -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        prepare_connection(&conn)?;
        migrations::apply_up_to(&conn, version)?;
        Ok(Self { conn })
    }

    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        backup_before_destructive_migration(path)?;
        let conn = Connection::open(path)?;
        prepare_connection(&conn)?;
        migrations::apply_pending(&conn)?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn schema_version(&self) -> anyhow::Result<u32> {
        Ok(migrations::read_schema_version(&self.conn))
    }
}

fn prepare_connection(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    let mut bak = path.as_os_str().to_os_string();
    bak.push(".bak");
    PathBuf::from(bak)
}

fn backup_before_destructive_migration(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let version = {
        let conn = Connection::open(path)?;
        migrations::read_schema_version(&conn)
    };
    if version < LATEST_SCHEMA_VERSION {
        // ponytail: copy the whole file before any pending migration; skip when already current.
        // Ceiling: no per-migration destructive flag. Add one when a later sprint ships a breaking schema.
        fs::copy(path, backup_path(path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AppPaths, Database, backup_path};

    #[test]
    fn explicit_data_home_wins() {
        let paths = AppPaths::from_data_home("C:/tmp/dexo-test".into());
        assert_eq!(paths.database.file_name().unwrap(), "dexo.db");
        assert_eq!(paths.config.file_name().unwrap(), "config.toml");
    }

    #[test]
    fn open_backs_up_existing_db_before_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE leftover(x INTEGER);")
                .unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 11);
        assert!(backup_path(&path).exists());
    }

    #[test]
    fn disk_full_backup_leaves_original_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(crate::migrations::MIGRATION_1).unwrap();
        }
        std::fs::create_dir(backup_path(&path)).unwrap();
        assert!(Database::open(&path).is_err());
        let conn = rusqlite::Connection::open(&path).unwrap();
        let version = crate::migrations::read_schema_version(&conn);
        assert_eq!(version, 1);
    }
}
