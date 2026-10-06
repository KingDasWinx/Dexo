//! The SDK answers the protocol for any driver: SQLite here, over in-process pipes, the
//! test playing Dexo's part line by line.
use std::io::{BufReader, PipeReader, PipeWriter};
use std::path::Path;
use std::sync::Arc;

use dexo_driver_api::{ConnectRequest, DbValue, QueryEvent};
use dexo_driver_sqlite::SqliteFactory;
use dexo_plugin_protocol::{
    QueryPage, Request, Response, SessionOpenParams, code, read_line, write_line,
};
use secrecy::SecretString;
use serde_json::{Value, json};

struct Wire {
    to: PipeWriter,
    from: BufReader<PipeReader>,
    next: u64,
}

impl Wire {
    fn start() -> Self {
        let (from, to) = dexo_plugin_sdk::serve_in_process(vec![Arc::new(SqliteFactory)]).unwrap();
        Self {
            to,
            from: BufReader::new(from),
            next: 0,
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Response {
        self.next += 1;
        write_line(
            &mut self.to,
            &Request::new(self.next, method, params).unwrap(),
        )
        .unwrap();
        let mut line = Vec::new();
        assert!(
            read_line(&mut self.from, &mut line).unwrap(),
            "the plugin hung up"
        );
        serde_json::from_slice(&line).unwrap()
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let response = self.call(method, params);
        assert!(response.error.is_none(), "{method}: {:?}", response.error);
        // A `null` result reads back as no result.
        response.result.unwrap_or(Value::Null)
    }

    fn open(&mut self, path: &Path) -> String {
        let request = ConnectRequest::new(
            path.to_string_lossy(),
            None,
            String::new(),
            SecretString::from(String::new()),
            false,
        );
        let params = serde_json::to_value(SessionOpenParams::new("sqlite", &request)).unwrap();
        self.ok("session/open", params)["session"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn execute(&mut self, session: &str, sql: &str) -> QueryPage {
        let page = self.ok(
            "query/execute",
            json!({"session": session, "args": {
                "query": uuid::Uuid::new_v4(), "sql": sql, "params": [], "row_limit": 0,
                "timeout_ms": 30000, "read_only": false, "mutating": false}}),
        );
        serde_json::from_value(page).unwrap()
    }
}

const COUNT_TO_1200: &str = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1200) SELECT i FROM n";

fn rows_in(page: &QueryPage) -> usize {
    page.events
        .iter()
        .map(|event| match event {
            QueryEvent::Rows(batch) => batch.rows.len(),
            _ => 0,
        })
        .sum()
}

#[test]
fn the_handshake_names_the_drivers_and_refuses_another_protocol() {
    let mut wire = Wire::start();
    let ok = wire.ok(
        "initialize",
        json!({"dexo_version": "1.5.0", "protocols": [1]}),
    );
    assert_eq!(ok["protocol"], 1);
    assert_eq!(ok["drivers"], json!(["sqlite"]));
    let refused = wire.call(
        "initialize",
        json!({"dexo_version": "9.0.0", "protocols": [2]}),
    );
    assert_eq!(refused.error.unwrap().code, code::INVALID_REQUEST);
}

#[test]
fn a_result_comes_in_pages_of_500_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut wire = Wire::start();
    let session = wire.open(&dir.path().join("a.db"));
    let first = wire.execute(&session, COUNT_TO_1200);
    assert_eq!(rows_in(&first), 500);
    assert!(!first.done);
    let cursor = first.cursor.unwrap();
    let fetch = |wire: &mut Wire| -> QueryPage {
        serde_json::from_value(wire.ok("query/fetch", json!({"cursor": cursor}))).unwrap()
    };
    assert_eq!(rows_in(&fetch(&mut wire)), 500);
    let last = fetch(&mut wire);
    assert_eq!(rows_in(&last), 200);
    assert!(last.done && last.cursor.is_none());
    assert!(
        last.events
            .iter()
            .any(|event| matches!(event, QueryEvent::Finished { .. }))
    );
    let gone = wire.call("query/fetch", json!({"cursor": cursor}));
    assert_eq!(gone.error.unwrap().code, code::UNKNOWN_CURSOR);
}

/// Review focus 2: a result read page by page holds nothing up -- the driver only works
/// when asked for the next page, and every request is answered on its own.
#[test]
fn a_long_result_does_not_hold_up_another_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.db");
    let mut wire = Wire::start();
    let long = wire.open(&path);
    let short = wire.open(&path);
    let first = wire.execute(&long, COUNT_TO_1200);
    assert!(!first.done, "the long result is still open");
    let answer = wire.execute(&short, "SELECT 42");
    assert!(answer.done);
    let rows: Vec<Vec<DbValue>> = answer
        .events
        .into_iter()
        .flat_map(|event| match event {
            QueryEvent::Rows(batch) => batch.rows,
            _ => vec![],
        })
        .collect();
    assert_eq!(rows, vec![vec![DbValue::I64(42)]]);
    wire.ok("query/close", json!({"cursor": first.cursor.unwrap()}));
}

#[test]
fn a_driver_error_comes_back_whole() {
    let dir = tempfile::tempdir().unwrap();
    let mut wire = Wire::start();
    let session = wire.open(&dir.path().join("a.db"));
    let page = wire.execute(&session, "SELEC 1");
    let Some(error) = page.error else {
        panic!("no error: {page:?}");
    };
    assert!(error.to_string().contains("syntax"), "{error}");
    let unknown = wire.call(
        "query/execute",
        json!({"session": "nope", "args": {"query": uuid::Uuid::new_v4(), "sql": "select 1",
            "params": [], "row_limit": 0, "timeout_ms": 1000, "read_only": false, "mutating": false}}),
    );
    assert_eq!(unknown.error.unwrap().code, code::UNKNOWN_SESSION);
}

#[test]
fn the_end_of_stdin_ends_the_server() {
    let Wire { to, mut from, .. } = Wire::start();
    drop(to);
    let mut line = Vec::new();
    // The server's end of the pipe closes once it has returned.
    assert!(!read_line(&mut from, &mut line).unwrap());
}

#[test]
fn a_session_answers_every_family_it_declares() {
    let dir = tempfile::tempdir().unwrap();
    let mut wire = Wire::start();
    let request = ConnectRequest::new(
        dir.path().join("a.db").to_string_lossy(),
        None,
        String::new(),
        SecretString::from(String::new()),
        false,
    );
    let opened = wire.ok(
        "session/open",
        serde_json::to_value(SessionOpenParams::new("sqlite", &request)).unwrap(),
    );
    for family in ["catalog", "data", "bulk", "explain", "transactions"] {
        assert!(
            opened["families"]
                .as_array()
                .unwrap()
                .contains(&json!(family)),
            "{family}: {opened}"
        );
    }
    let session = opened["session"].as_str().unwrap().to_string();
    wire.execute(
        &session,
        "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)",
    );
    let on = |args: Value| json!({"session": session, "args": args});
    let inserted = wire.ok(
        "bulk/insert_batch",
        on(
            json!([{"catalog": null, "schema": "main", "object": "t"}, ["id", "name"],
            [[{"t": "i64", "v": 1}, {"t": "text", "v": "ada"}]]]),
        ),
    );
    assert_eq!(inserted, json!(1));
    let databases = wire.ok("catalog/databases", on(Value::Null));
    assert!(
        databases.as_array().is_some_and(|names| !names.is_empty()),
        "{databases}"
    );
    let began = wire.ok("tx/begin", on(json!("read_write")));
    assert_eq!(began["state"], "active");
    let rolled = wire.ok("tx/rollback", on(Value::Null));
    assert_eq!(rolled["state"], "idle");
    let missing = wire.call("teleport/now", on(Value::Null));
    assert_eq!(missing.error.unwrap().code, code::METHOD_NOT_FOUND);
}
