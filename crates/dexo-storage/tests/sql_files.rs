use dexo_storage::sql_files;
use tempfile::tempdir;

#[test]
fn write_sql_file_persists_content_without_leaving_a_temp_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("query.sql");
    let content = "SELECT 1;\n";

    sql_files::write_sql_file(&path, content).unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "temp file leaked"
    );
}

#[test]
fn concurrent_writes_do_not_share_a_temp_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("query.sql");

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
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "every write must clean up its own temp file"
    );
}
