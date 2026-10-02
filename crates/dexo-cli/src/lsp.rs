//! `dexo lsp`: Dexo's SQL completion, diagnostics and formatting for any editor, as a
//! language server on stdin and stdout. JSON-RPC with Content-Length framing, written
//! here: the protocol needs a handful of messages, not a framework.
//!
//! A document's connection is `--connection`, or a first line `-- dexo: connection=name`.
//! Its catalog is the one Dexo cached for that connection: the server never dials a
//! database, so a slow one never holds an answer up.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use dexo_app::catalog_service::{SnapshotCatalog, known_objects};
use dexo_sql::{CompletionKind, Diagnoser, Dialect, KnownObjects};
use serde_json::{Value, json};

/// What a connection's documents are checked and completed against.
struct Schema {
    catalog: SnapshotCatalog,
    known: Option<KnownObjects>,
    dialect: Dialect,
}

/// A connection's schema as last read, and the state of Dexo's database file then.
struct Cached {
    stamp: Option<(std::time::SystemTime, u64)>,
    schema: Option<Schema>,
}

struct Document {
    text: String,
    diagnoser: Diagnoser,
}

pub struct Server {
    connection: Option<String>,
    database: Option<std::path::PathBuf>,
    documents: HashMap<String, Document>,
    /// Read per connection, again whenever Dexo's database file changes.
    schemas: HashMap<String, Cached>,
    /// Whether the client asked the server to shut down; `exit` without it is an error.
    shut_down: bool,
}

impl Server {
    pub fn new(connection: Option<String>, database: Option<std::path::PathBuf>) -> Self {
        Self {
            connection,
            database,
            documents: HashMap::new(),
            schemas: HashMap::new(),
            shut_down: false,
        }
    }

    /// Answers what came in: the response to a request (every id gets one) and the
    /// notifications it caused. `None` once the client said `exit`.
    pub fn handle(&mut self, message: &Value) -> Option<Vec<Value>> {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let mut out = Vec::new();
        let result: Result<Value, (i64, String)> = match method {
            "exit" => return None,
            "initialize" => Ok(json!({
                "capabilities": {
                    "textDocumentSync": 1,
                    "completionProvider": { "triggerCharacters": ["."] },
                    "documentFormattingProvider": true
                },
                "serverInfo": { "name": "dexo", "version": env!("CARGO_PKG_VERSION") }
            })),
            "shutdown" => {
                self.shut_down = true;
                Ok(Value::Null)
            }
            "textDocument/didOpen" => {
                let document = &params["textDocument"];
                if let (Some(uri), Some(text)) =
                    (document["uri"].as_str(), document["text"].as_str())
                {
                    self.documents.insert(
                        uri.to_string(),
                        Document {
                            text: text.to_string(),
                            diagnoser: Diagnoser::default(),
                        },
                    );
                    out.push(self.diagnostics(uri));
                }
                Ok(Value::Null)
            }
            "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                // Full sync: the last change is the whole text.
                let text = params["contentChanges"]
                    .as_array()
                    .and_then(|changes| changes.last())
                    .and_then(|change| change["text"].as_str());
                if let (Some(document), Some(text)) = (self.documents.get_mut(uri), text) {
                    document.text = text.to_string();
                    out.push(self.diagnostics(uri));
                }
                Ok(Value::Null)
            }
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                self.documents.remove(uri);
                out.push(json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": { "uri": uri, "diagnostics": [] }
                }));
                Ok(Value::Null)
            }
            "textDocument/completion" => Ok(self.completion(&params)),
            "textDocument/formatting" => self.formatting(&params),
            _ => Err((-32601, format!("{method} is not supported"))),
        };
        if let Some(id) = id {
            out.insert(
                0,
                match result {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err((code, message)) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": code, "message": message }
                    }),
                },
            );
        }
        Some(out)
    }

    /// The connection a document names on its first line, or the server's.
    fn connection_of(&self, text: &str) -> Option<String> {
        text.lines()
            .next()
            .and_then(|line| line.trim().strip_prefix("--"))
            .and_then(|rest| rest.trim().strip_prefix("dexo:"))
            .and_then(|rest| rest.trim().strip_prefix("connection="))
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .or_else(|| self.connection.clone())
    }

    /// The schema of `connection`, read from the cache when it is first asked for and
    /// again once Dexo's database file has changed, so a catalog cached or refreshed
    /// while the editor runs -- or a first one after none -- is seen without a restart.
    fn schema(&mut self, connection: Option<&str>) -> Option<&Schema> {
        let connection = connection?;
        let stamp = self
            .database
            .as_ref()
            .and_then(|path| std::fs::metadata(path).ok())
            .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
        if self
            .schemas
            .get(connection)
            .is_none_or(|cached| cached.stamp != stamp)
        {
            let schema = self.load(connection);
            self.schemas
                .insert(connection.to_string(), Cached { stamp, schema });
        }
        self.schemas
            .get(connection)
            .and_then(|cached| cached.schema.as_ref())
    }

    /// The connection's cached catalog: the command line's cache first (by the
    /// connection's id and database), then the workbench's (by its name). Dexo's
    /// database is only read: a server an editor started never migrates, creates or
    /// archives it, and a lock held by Dexo means no catalog until the file changes.
    fn load(&self, connection: &str) -> Option<Schema> {
        let path = self.database.as_ref()?;
        let db = dexo_storage::Database::open_read_only(path).ok()?;
        let profile = dexo_storage::ConnectionRepository::new(db.connection())
            .get_by_name(connection)
            .ok()??;
        let cache = dexo_storage::CatalogCache::new(db.connection());
        let database = crate::run::catalog_database_name(&profile);
        let mut objects = cache
            .load_latest(&profile.id.0.to_string(), &database)
            .unwrap_or_default();
        if objects.is_empty() {
            objects = cache
                .load_latest(connection, connection)
                .unwrap_or_default();
        }
        let known = (!objects.is_empty()).then(|| known_objects(&objects));
        Some(Schema {
            catalog: SnapshotCatalog::new(objects),
            known,
            dialect: dexo_app::dialect_for_driver(&profile.driver),
        })
    }

    fn dialect_of(&mut self, text: &str) -> Dialect {
        let connection = self.connection_of(text);
        self.schema(connection.as_deref())
            .map_or(Dialect::Postgres, |schema| schema.dialect)
    }

    fn diagnostics(&mut self, uri: &str) -> Value {
        let Some(text) = self
            .documents
            .get(uri)
            .map(|document| document.text.clone())
        else {
            return Value::Null;
        };
        let connection = self.connection_of(&text);
        self.schema(connection.as_deref());
        let schema = connection
            .as_deref()
            .and_then(|name| self.schemas.get(name))
            .and_then(|cached| cached.schema.as_ref());
        let (dialect, known) = schema.map_or((Dialect::Postgres, None), |schema| {
            (schema.dialect, schema.known.as_ref())
        });
        let Some(document) = self.documents.get_mut(uri) else {
            return Value::Null;
        };
        // No cursor to wait for: an editor does not say where it is.
        let found = document
            .diagnoser
            .diagnose(&text, dialect, known, usize::MAX);
        let diagnostics: Vec<Value> = found
            .into_iter()
            .map(|diagnostic| {
                let range = diagnostic.byte_range.unwrap_or(0..0);
                json!({
                    "range": {
                        "start": position_of(&text, range.start),
                        "end": position_of(&text, range.end),
                    },
                    "severity": 1,
                    "source": "dexo",
                    "message": diagnostic.message,
                })
            })
            .collect();
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": uri, "diagnostics": diagnostics }
        })
    }

    fn completion(&mut self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let Some(text) = self
            .documents
            .get(uri)
            .map(|document| document.text.clone())
        else {
            return json!([]);
        };
        let cursor = offset_of(&text, &params["position"]);
        let connection = self.connection_of(&text);
        let empty = SnapshotCatalog::new(Vec::new());
        let (catalog, dialect) = match self.schema(connection.as_deref()) {
            Some(schema) => (&schema.catalog, schema.dialect),
            None => (&empty, Dialect::Postgres),
        };
        let items: Vec<Value> = dexo_sql::complete(&text, cursor, catalog, dialect)
            .into_iter()
            .map(|item| {
                let kind = match item.kind {
                    CompletionKind::Keyword => 14,
                    CompletionKind::Table => 7,
                    CompletionKind::Column => 5,
                    CompletionKind::Alias => 6,
                    CompletionKind::Function => 3,
                    CompletionKind::Snippet => 15,
                };
                let mut out = json!({ "label": item.label, "kind": kind });
                if let Some(detail) = item.detail.or(item.signature) {
                    out["detail"] = json!(detail);
                }
                out
            })
            .collect();
        json!(items)
    }

    fn formatting(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let Some(text) = self
            .documents
            .get(uri)
            .map(|document| document.text.clone())
        else {
            return Ok(json!([]));
        };
        let dialect = self.dialect_of(&text);
        let options = &params["options"];
        let indent = if options["insertSpaces"].as_bool() == Some(false) {
            dexo_sql::Indent::Tab
        } else {
            let width = options["tabSize"]
                .as_u64()
                .map_or(2, |width| width.clamp(1, 16));
            dexo_sql::Indent::Spaces(width as u8)
        };
        let formatted = dexo_sql::format_sql_with(&text, dialect, indent)
            .map_err(|error| (-32603, error.to_string()))?;
        if formatted == text {
            return Ok(json!([]));
        }
        Ok(json!([{
            "range": { "start": position_of(&text, 0), "end": position_of(&text, text.len()) },
            "newText": formatted,
        }]))
    }
}

/// An LSP position -- a line, and a column in UTF-16 code units -- as a byte offset.
fn offset_of(text: &str, position: &Value) -> usize {
    let line = position["line"].as_u64().unwrap_or(0) as usize;
    let column = position["character"].as_u64().unwrap_or(0) as usize;
    let mut start = 0;
    for _ in 0..line {
        match text[start..].find('\n') {
            Some(at) => start += at + 1,
            None => return text.len(),
        }
    }
    let end = text[start..].find('\n').map_or(text.len(), |at| start + at);
    let mut units = 0;
    for (at, ch) in text[start..end].char_indices() {
        if units >= column {
            return start + at;
        }
        units += ch.len_utf16();
    }
    end
}

/// A byte offset as an LSP position.
fn position_of(text: &str, offset: usize) -> Value {
    let offset = offset.min(text.len());
    let before = &text[..text.floor_char_boundary(offset)];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |at| at + 1);
    let character: usize = before[line_start..].chars().map(char::len_utf16).sum();
    json!({ "line": line, "character": character })
}

/// What a header block and its body held: a message, or why it is none.
enum Incoming {
    Message(Value),
    /// The body was not JSON; the length still said where the next message starts.
    Malformed(String),
    /// No Content-Length, so nothing says where the body ends or the next one starts.
    Unframed,
}

/// One message from `input`, or `None` at its end.
fn read_message(input: &mut impl BufRead) -> std::io::Result<Option<Incoming>> {
    let mut length = None;
    loop {
        let mut header = String::new();
        if input.read_line(&mut header)? == 0 {
            return Ok(None);
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = length else {
        return Ok(Some(Incoming::Unframed));
    };
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    Ok(Some(match serde_json::from_slice(&body) {
        Ok(message) => Incoming::Message(message),
        Err(error) => Incoming::Malformed(error.to_string()),
    }))
}

fn write_message(output: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let body = message.to_string();
    write!(output, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    output.flush()
}

/// Serves `input` to `output` until the client says `exit` or the input ends, and says
/// whether the client shut the server down first: the exit code is 1 when it did not.
/// A body that is not JSON is answered with a parse error and the next message read; a
/// message without a length is answered too, and ends the stream, which can no longer
/// be followed.
pub fn serve(
    server: &mut Server,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> std::io::Result<bool> {
    let parse_error = |message: &str| {
        json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": { "code": -32700, "message": message }
        })
    };
    while let Some(incoming) = read_message(input)? {
        let message = match incoming {
            Incoming::Message(message) => message,
            Incoming::Malformed(error) => {
                write_message(output, &parse_error(&error))?;
                continue;
            }
            Incoming::Unframed => {
                write_message(output, &parse_error("a message without Content-Length"))?;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "a message without Content-Length: the stream cannot be followed",
                ));
            }
        };
        let Some(replies) = server.handle(&message) else {
            return Ok(server.shut_down);
        };
        for reply in replies.iter().filter(|reply| !reply.is_null()) {
            write_message(output, reply)?;
        }
    }
    Ok(server.shut_down)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{Server, offset_of, position_of, serve};

    fn framed(messages: &[Value]) -> Vec<u8> {
        let mut out = Vec::new();
        for message in messages {
            let body = message.to_string();
            out.extend(format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes());
        }
        out
    }

    fn replies(output: &[u8]) -> Vec<Value> {
        let mut input = std::io::BufReader::new(output);
        let mut out = Vec::new();
        while let Some(incoming) = super::read_message(&mut input).unwrap() {
            let super::Incoming::Message(message) = incoming else {
                panic!("the server wrote a message that does not parse");
            };
            out.push(message);
        }
        out
    }

    /// Positions count UTF-16 code units: an emoji is two, an accented letter one.
    #[test]
    fn positions_count_utf16_units() {
        let text = "select 'é😀' as x\nfrom t";
        let after_emoji = text.find("' as").unwrap();
        assert_eq!(
            position_of(text, after_emoji),
            json!({"line": 0, "character": 11})
        );
        assert_eq!(
            offset_of(text, &json!({"line": 0, "character": 11})),
            after_emoji
        );
        assert_eq!(
            offset_of(text, &json!({"line": 1, "character": 5})),
            text.len() - 1
        );
        assert_eq!(
            position_of(text, text.len()),
            json!({"line": 1, "character": 6})
        );
    }

    /// Every request is answered by its id, an unknown one with an error; diagnostics
    /// come with an open document; formatting rewrites it; `exit` ends the loop.
    #[test]
    fn a_session_from_initialize_to_exit() {
        let uri = "file:///tmp/q.sql";
        let input = framed(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
            json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
                "textDocument": {"uri": uri, "languageId": "sql", "version": 1, "text": "select * from t where"}
            }}),
            json!({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [{"text": "select a,b from t where a=1"}]
            }}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/formatting", "params": {
                "textDocument": {"uri": uri}, "options": {"tabSize": 4, "insertSpaces": true}
            }}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/completion", "params": {
                "textDocument": {"uri": uri}, "position": {"line": 0, "character": 3}
            }}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "workspace/symbol", "params": {}}),
            json!({"jsonrpc": "2.0", "id": 5, "method": "shutdown"}),
            json!({"jsonrpc": "2.0", "method": "exit"}),
            json!({"jsonrpc": "2.0", "id": 6, "method": "shutdown"}),
        ]);
        let mut output = Vec::new();
        let mut server = Server::new(None, None);
        let shut_down = serve(
            &mut server,
            &mut std::io::BufReader::new(&input[..]),
            &mut output,
        )
        .unwrap();
        assert!(shut_down);
        let replies = replies(&output);
        let answered: Vec<i64> = replies
            .iter()
            .filter_map(|reply| reply["id"].as_i64())
            .collect();
        assert_eq!(answered, [1, 2, 3, 4, 5], "{replies:?}");
        assert!(replies[0]["result"]["capabilities"]["documentFormattingProvider"] == json!(true));
        let published: Vec<&Value> = replies
            .iter()
            .filter(|reply| reply["method"] == "textDocument/publishDiagnostics")
            .collect();
        assert_eq!(published.len(), 2);
        assert!(
            !published[0]["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            published[1]["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let edits = &replies.iter().find(|reply| reply["id"] == 2).unwrap()["result"];
        assert!(
            edits[0]["newText"].as_str().unwrap().contains("SELECT"),
            "{edits}"
        );
        let completion = &replies.iter().find(|reply| reply["id"] == 3).unwrap()["result"];
        assert!(
            completion.as_array().is_some_and(|items| !items.is_empty()),
            "{completion}"
        );
        let unknown = replies.iter().find(|reply| reply["id"] == 4).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    /// A body that is not JSON gets a parse error and the server reads on; one without a
    /// length gets one too and ends the stream; `exit` with no `shutdown` before it is
    /// not a clean end.
    #[test]
    fn a_broken_message_is_answered_with_a_parse_error() {
        let mut input = b"Content-Length: 9\r\n\r\n{not json".to_vec();
        input.extend(framed(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            json!({"jsonrpc": "2.0", "method": "exit"}),
        ]));
        let mut output = Vec::new();
        let shut_down = serve(
            &mut Server::new(None, None),
            &mut std::io::BufReader::new(&input[..]),
            &mut output,
        )
        .unwrap();
        assert!(!shut_down);
        let answers = replies(&output);
        assert_eq!(answers[0]["error"]["code"], -32700, "{answers:?}");
        assert_eq!(answers[0]["id"], Value::Null);
        assert_eq!(answers[1]["id"], 1, "{answers:?}");

        let input = b"Content-Type: x\r\n\r\n{}".to_vec();
        let mut output = Vec::new();
        assert!(
            serve(
                &mut Server::new(None, None),
                &mut std::io::BufReader::new(&input[..]),
                &mut output,
            )
            .is_err()
        );
        assert_eq!(replies(&output)[0]["error"]["code"], -32700);
    }

    /// Formatting indents the way the editor asks: its tab size in spaces, or tabs.
    #[test]
    fn formatting_follows_the_editor_options() {
        let mut server = Server::new(None, None);
        let uri = "file:///f.sql";
        server.handle(
            &json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
                "textDocument": {"uri": uri, "languageId": "sql", "version": 1, "text": "select a from t"}
            }}),
        );
        let mut formatted = |options: Value| {
            server
                .handle(
                    &json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/formatting", "params": {
                        "textDocument": {"uri": uri}, "options": options
                    }}),
                )
                .unwrap()[0]["result"][0]["newText"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(
            formatted(json!({"tabSize": 4, "insertSpaces": true})),
            "SELECT\n    a\nFROM\n    t"
        );
        assert_eq!(
            formatted(json!({"tabSize": 8, "insertSpaces": false})),
            "SELECT\n\ta\nFROM\n\tt"
        );
    }

    /// The first line names the document's connection over the server's.
    #[test]
    fn the_first_line_names_the_connection() {
        let server = Server::new(Some("default".into()), None);
        assert_eq!(
            server
                .connection_of("-- dexo: connection=shop\nselect 1")
                .as_deref(),
            Some("shop")
        );
        assert_eq!(server.connection_of("select 1").as_deref(), Some("default"));
    }

    /// A catalog cached after the server first looked, when there was none, is offered
    /// without restarting the server.
    #[test]
    fn a_catalog_cached_later_is_seen() {
        use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        let profile = dexo_app::ConnectionProfile::new(
            dexo_app::connection_profile::ConnectionId(uuid::Uuid::new_v4()),
            None,
            "shop",
            "postgres",
            "local",
            json!({"host": "h", "database": "shop"}),
            dexo_app::connection_profile::SecretRef::new("ref".into()),
        );
        let db = dexo_storage::Database::open(&path).unwrap();
        dexo_storage::ConnectionRepository::new(db.connection())
            .save(&profile)
            .unwrap();
        let mut server = Server::new(Some("shop".into()), Some(path));
        let uri = "file:///q.sql";
        server.handle(
            &json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
                "textDocument": {"uri": uri, "languageId": "sql", "version": 1, "text": "select * from ord"}
            }}),
        );
        let labels = |server: &mut Server| -> Vec<String> {
            server
                .handle(
                    &json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/completion", "params": {
                        "textDocument": {"uri": uri}, "position": {"line": 0, "character": 17}
                    }}),
                )
                .unwrap()[0]["result"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|item| item["label"].as_str().map(str::to_string))
                .collect()
        };
        assert!(!labels(&mut server).contains(&"orders".to_string()));
        dexo_storage::CatalogCache::new(db.connection())
            .replace_snapshot(
                &profile.id.0.to_string(),
                "shop",
                &[CatalogObject::new(
                    ObjectId::new("table:orders"),
                    ObjectKind::Table,
                    QualifiedName::new(Some("shop"), Some("public"), "orders"),
                    None,
                )],
            )
            .unwrap();
        assert!(labels(&mut server).contains(&"orders".to_string()));
    }

    /// Completion and diagnostics read the connection's cached catalog: its tables are
    /// offered, and one it does not have is reported.
    #[test]
    fn the_cached_catalog_completes_and_checks() {
        use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        let profile = dexo_app::ConnectionProfile::new(
            dexo_app::connection_profile::ConnectionId(uuid::Uuid::new_v4()),
            None,
            "shop",
            "postgres",
            "local",
            json!({"host": "h", "database": "shop"}),
            dexo_app::connection_profile::SecretRef::new("ref".into()),
        );
        {
            let db = dexo_storage::Database::open(&path).unwrap();
            dexo_storage::ConnectionRepository::new(db.connection())
                .save(&profile)
                .unwrap();
            let table = |name: &str| {
                CatalogObject::new(
                    ObjectId::new(format!("table:{name}")),
                    ObjectKind::Table,
                    QualifiedName::new(Some("shop"), Some("public"), name),
                    None,
                )
            };
            dexo_storage::CatalogCache::new(db.connection())
                .replace_snapshot(
                    &profile.id.0.to_string(),
                    "shop",
                    &[table("orders"), table("customers")],
                )
                .unwrap();
        }
        let mut server = Server::new(None, Some(path));
        let uri = "file:///q.sql";
        let text = "-- dexo: connection=shop\nselect * from ord";
        server.handle(
            &json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
                "textDocument": {"uri": uri, "languageId": "sql", "version": 1, "text": text}
            }}),
        );
        let replies = server
            .handle(
                &json!({"jsonrpc": "2.0", "id": 7, "method": "textDocument/completion", "params": {
                    "textDocument": {"uri": uri}, "position": {"line": 1, "character": 17}
                }}),
            )
            .unwrap();
        let labels: Vec<&str> = replies[0]["result"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["label"].as_str())
            .collect();
        assert!(labels.contains(&"orders"), "{labels:?}");
        let replies = server
            .handle(
                &json!({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [{"text": "-- dexo: connection=shop\nselect * from invoices"}]
                }}),
            )
            .unwrap();
        let diagnostics = replies[0]["params"]["diagnostics"].as_array().unwrap();
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic["message"]
                .as_str()
                .unwrap_or("")
                .contains("invoices")),
            "{diagnostics:?}"
        );
    }
}
