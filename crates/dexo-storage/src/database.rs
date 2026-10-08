use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::migrations::{self, LATEST_SCHEMA_VERSION};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub database: PathBuf,
}

impl AppPaths {
    pub fn from_data_home(data_dir: PathBuf) -> Self {
        Self {
            database: data_dir.join("dexo.db"),
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
        refuse_newer_schema(path)?;
        backup_before_destructive_migration(path)?;
        let conn = Connection::open(path)?;
        prepare_connection(&conn)?;
        migrations::apply_pending(&conn)?;
        Ok(Self { conn })
    }

    /// An existing database opened only to be read, by a process that must leave it as
    /// it is -- the language server an editor keeps running: nothing is created, migrated
    /// or archived, and a lock another process holds is an error at once rather than a
    /// wait. A database at a schema version other than this build's is refused, since
    /// its tables may not read the way this build expects.
    pub fn open_read_only(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(50))?;
        let version = migrations::read_schema_version(&conn);
        anyhow::ensure!(
            version == LATEST_SCHEMA_VERSION,
            "the database is at schema version {version}, and this build reads {LATEST_SCHEMA_VERSION}"
        );
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
    // Readers do not wait for a writer: the MCP server writing its audit, a catalog
    // snapshot being saved, another Dexo. With the rollback journal each read waited out
    // the write, up to the five-second busy timeout.
    conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
        row.get::<_, String>(0)
    })?;
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    let mut bak = path.as_os_str().to_os_string();
    bak.push(".bak");
    PathBuf::from(bak)
}

fn read_schema_version_at_path(path: &Path) -> anyhow::Result<u32> {
    let conn = Connection::open(path)?;
    Ok(migrations::read_schema_version(&conn))
}

/// A database a newer Dexo wrote is refused, and left as it is. Every migration here is
/// additive, but an older build cannot know that; moving the file aside and restoring a
/// backup forked the data under the processes still writing to it.
fn refuse_newer_schema(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let version = read_schema_version_at_path(path)?;
    anyhow::ensure!(
        version <= LATEST_SCHEMA_VERSION,
        "{} was written by a newer Dexo (schema {version}); this build reads schema {LATEST_SCHEMA_VERSION}. Run the newer Dexo, or upgrade this one: an older `dexo` on PATH and a newer one built elsewhere both open this file.",
        path.display()
    );
    Ok(())
}

/// The copies an older Dexo moved aside before it refused a newer database:
/// `dexo.db.unsupported-v*` beside it. They are the person's data, so they are named, not
/// deleted.
pub fn leftover_archives(data_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(data_dir) else {
        return Vec::new();
    };
    let mut archives: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(".unsupported-v"))
        })
        .collect();
    archives.sort();
    archives
}

fn backup_before_destructive_migration(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let version = read_schema_version_at_path(path)?;
    if version < LATEST_SCHEMA_VERSION {
        // ponytail: copy the whole file before any pending migration; skip when already current.
        // Ceiling: no per-migration destructive flag. Add one when a later sprint ships a breaking schema.
        // Copied by SQLite, the write-ahead log included: a write another process has
        // open -- the MCP server -- is in the log, not yet in the file a plain copy took.
        let backup = backup_path(path);
        if backup.exists() {
            fs::remove_file(&backup)?;
        }
        Connection::open(path)?.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{AppPaths, Database, backup_path, leftover_archives};

    /// A database a newer Dexo wrote is left exactly as it is: the older build refuses
    /// it and says to upgrade. Archiving it and restoring the backup forked the data under
    /// the processes still writing to it.
    #[test]
    fn open_refuses_a_database_from_a_newer_dexo_and_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        Database::open(&path).unwrap();
        fs::copy(&path, backup_path(&path)).unwrap();
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES(23, datetime('now'))",
                [],
            )
            .unwrap();
        }
        let files = || {
            let mut names: Vec<_> = fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        let before = files();
        let Err(error) = Database::open(&path) else {
            panic!("a database from a newer Dexo was opened");
        };
        let error = error.to_string();
        assert!(
            error.contains("newer Dexo") && error.contains("schema 23"),
            "{error}"
        );
        assert_eq!(files(), before, "nothing moved, copied or created");
        assert!(leftover_archives(dir.path()).is_empty());
        let conn = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(crate::migrations::read_schema_version(&conn), 23);
    }

    /// Opening to read never creates, migrates or archives anything, and does not wait
    /// on a lock another connection holds.
    #[test]
    fn open_read_only_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("none/dexo.db");
        assert!(Database::open_read_only(&missing).is_err());
        assert!(!missing.parent().unwrap().exists());

        let path = dir.path().join("dexo.db");
        Database::open(&path).unwrap();
        let db = Database::open_read_only(&path).unwrap();
        assert!(
            db.connection()
                .execute("DELETE FROM schema_migrations", [])
                .is_err()
        );
        drop(db);

        // A writer holding the database is not waited for: in WAL a reader reads past it.
        let writer = rusqlite::Connection::open(&path).unwrap();
        writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let started = std::time::Instant::now();
        let _ = Database::open_read_only(&path);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        writer.execute_batch("ROLLBACK").unwrap();

        writer
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES(23, datetime('now'))",
                [],
            )
            .unwrap();
        assert!(Database::open_read_only(&path).is_err());
        assert!(path.exists());
        assert!(leftover_archives(dir.path()).is_empty());
    }

    /// A reader is not held up by a write in progress on another connection.
    #[test]
    fn a_read_does_not_wait_for_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        Database::open(&path).unwrap();
        let writer = Database::open(&path).unwrap();
        writer
            .connection()
            .execute_batch("BEGIN EXCLUSIVE; DELETE FROM sql_history;")
            .unwrap();
        let reader = Database::open(&path).unwrap();
        let started = std::time::Instant::now();
        let count: i64 = reader
            .connection()
            .query_row("SELECT count(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(count > 0);
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        writer.connection().execute_batch("COMMIT;").unwrap();
    }

    #[test]
    fn explicit_data_home_wins() {
        let paths = AppPaths::from_data_home("C:/tmp/dexo-test".into());
        assert_eq!(paths.database.file_name().unwrap(), "dexo.db");
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
        assert_eq!(db.schema_version().unwrap(), 19);
        assert!(backup_path(&path).exists());
    }

    /// The backup holds what is still in the write-ahead log: another process has the
    /// database open, so nothing was checkpointed into the file.
    #[test]
    fn the_backup_before_a_migration_has_what_the_log_holds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        let keeper = rusqlite::Connection::open(&path).unwrap();
        keeper
            .execute_batch(crate::migrations::MIGRATION_1)
            .unwrap();
        keeper
            .query_row("PRAGMA journal_mode = WAL", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap();
        keeper
            .execute_batch("CREATE TABLE marker(x INTEGER); INSERT INTO marker VALUES(42);")
            .unwrap();

        Database::open(&path).unwrap();

        let backup = rusqlite::Connection::open(backup_path(&path)).unwrap();
        let kept: i64 = backup
            .query_row("SELECT x FROM marker", [], |row| row.get(0))
            .unwrap();
        assert_eq!(kept, 42);
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
