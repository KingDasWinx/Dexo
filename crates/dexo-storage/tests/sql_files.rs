use dexo_storage::{AppPaths, sql_files};
use tempfile::tempdir;

#[test]
fn console_sql_is_created_under_connection_dir() {
    let root = tempdir().unwrap();
    let paths = AppPaths::from_data_home(root.path().to_path_buf());
    let dir = sql_files::ensure_connection_sql_dir(&paths, "11111111-1111-1111-1111-111111111111")
        .unwrap();
    let console = sql_files::ensure_console_sql(&dir).unwrap();
    assert!(console.ends_with("console.sql"));
    assert_eq!(std::fs::read_to_string(&console).unwrap(), "");
    assert_eq!(sql_files::list_sql_files(&dir).unwrap().len(), 1);
}

#[test]
fn write_sql_file_persists_content_and_lists() {
    let root = tempdir().unwrap();
    let paths = AppPaths::from_data_home(root.path().to_path_buf());
    let dir = sql_files::ensure_connection_sql_dir(&paths, "22222222-2222-2222-2222-222222222222")
        .unwrap();
    let path = dir.join("query.sql");
    let content = "SELECT 1;\n";

    sql_files::write_sql_file(&path, content).unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    let listed = sql_files::list_sql_files(&dir).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0], path);
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        1,
        "temp file leaked"
    );
}

#[test]
fn concurrent_writes_do_not_share_a_temp_file() {
    let root = tempdir().unwrap();
    let paths = AppPaths::from_data_home(root.path().to_path_buf());
    let dir = sql_files::ensure_connection_sql_dir(&paths, "33333333-3333-3333-3333-333333333333")
        .unwrap();
    let path = dir.join("console.sql");

    let writers: Vec<_> = (0..8)
        .map(|i| {
            let path = path.clone();
            std::thread::spawn(move || sql_files::write_sql_file(&path, &format!("SELECT {i};\n")))
        })
        .collect();
    for writer in writers {
        writer.join().unwrap().unwrap();
    }

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.starts_with("SELECT ") && content.ends_with(";\n"),
        "{content}"
    );
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        1,
        "every write must clean up its own temp file"
    );
}
