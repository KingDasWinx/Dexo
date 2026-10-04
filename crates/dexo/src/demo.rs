//! `dexo --demo`: a seeded SQLite shop, opened as a temporary connection named `demo`.

use dexo_app::connection_url::UrlConnection;

const SEED: &str = include_str!("demo.sql");

/// Recreates the store under Dexo's own data directory -- per user, unlike a fixed name
/// in the shared temp directory, which anyone could plant a link at -- so every demo
/// starts from the same rows.
pub fn create() -> anyhow::Result<UrlConnection> {
    let paths = dexo_storage::AppPaths::discover()?;
    std::fs::create_dir_all(&paths.data_dir)?;
    // ponytail: one store per user; two demos at once reset each other's.
    let path = paths.data_dir.join("demo.sqlite3");
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let mut file = path.clone().into_os_string();
        file.push(suffix);
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    rusqlite::Connection::open(&path)?.execute_batch(SEED)?;
    let mut connection = dexo_app::connection_url::file("sqlite", &path)?;
    connection.profile.name = "demo".into();
    // Rebuilt on every run: the workbench will not save it as a connection of its own.
    connection.profile.config["demo"] = true.into();
    Ok(connection)
}

#[cfg(test)]
mod tests {
    /// The seed runs on the SQLite the binary bundles, and its rows hold together.
    #[test]
    fn the_seed_builds_a_consistent_shop() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(super::SEED).unwrap();
        let count = |table: &str| -> i64 {
            db.query_row(&format!("select count(*) from {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
        };
        assert_eq!(
            [
                count("customers"),
                count("products"),
                count("orders"),
                count("order_items")
            ],
            [120, 40, 300, 750]
        );
        let dangling: i64 = db
            .query_row("select count(*) from pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(dangling, 0);
    }
}
