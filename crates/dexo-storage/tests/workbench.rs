use dexo_storage::{Database, HistoryRepository, SnippetRepository};

#[test]
fn history_and_snippet_round_trip() {
    let db = Database::open_in_memory().unwrap();
    let history = HistoryRepository::new(db.connection());
    history
        .insert("h1", None, "select * from users where email=:email")
        .unwrap();
    history.prune(10).unwrap();
    assert_eq!(history.count().unwrap(), 1);
    let snippets = SnippetRepository::new(db.connection());
    snippets.upsert("s1", "sel", "select ${1:col}").unwrap();
    assert_eq!(
        snippets.get_body("s1").unwrap().as_deref(),
        Some("select ${1:col}")
    );
}
