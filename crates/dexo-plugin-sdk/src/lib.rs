//! Turns any `ConnectionFactory` into a driver plugin. `serve` answers the plugin
//! protocol on stdin and stdout until stdin closes, so a plugin never outlives the Dexo
//! that started it, even one that died without stopping it.

mod families;

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use dexo_driver_api::{
    ConnectionFactory, DbValue, QueryEvent, QueryId, QueryStream, RowBatch, Session,
};
use dexo_plugin_protocol::{
    self as protocol, CancelRequest, Cursor, InitializeParams, InitializeResult, OnSession,
    QueryExecute, QueryPage, Request, Response, RpcError, SessionOpenParams, SessionOpenResult,
    code, method,
};
use futures_util::StreamExt;
use futures_util::future::{AbortHandle, Abortable};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Serves `factories` on stdin and stdout, and returns when stdin reaches its end.
/// `version` is the plugin's own, said in the handshake.
pub fn serve(factories: Vec<Arc<dyn ConnectionFactory>>, version: &str) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime for the plugin");
    runtime.block_on(serve_io(
        BufReader::new(std::io::stdin()),
        std::io::stdout(),
        factories,
        version.to_string(),
    ));
}

/// Serves `factories` on a thread of this process, and returns the host's ends of the
/// pipes: what the plugin writes, to read, and what it reads, to write. For tests.
pub fn serve_in_process(
    factories: Vec<Arc<dyn ConnectionFactory>>,
) -> std::io::Result<(std::io::PipeReader, std::io::PipeWriter)> {
    let (host_reader, plugin_writer) = std::io::pipe()?;
    let (plugin_reader, host_writer) = std::io::pipe()?;
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("a tokio runtime for the plugin");
        runtime.block_on(serve_io(
            BufReader::new(plugin_reader),
            plugin_writer,
            factories,
            "0.0.0".into(),
        ));
    });
    Ok((host_reader, host_writer))
}

/// Answers every request in a task of its own, so a cancel is heard while a query runs
/// and one session's page never waits for another's.
pub async fn serve_io(
    reader: impl BufRead + Send + 'static,
    writer: impl Write + Send + 'static,
    factories: Vec<Arc<dyn ConnectionFactory>>,
    version: String,
) {
    let (lines_tx, mut lines) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || read_lines(reader, lines_tx));
    let (out, responses) = std::sync::mpsc::channel::<Response>();
    std::thread::spawn(move || write_lines(writer, responses));
    let server = Arc::new(Server {
        factories,
        version,
        sessions: Mutex::default(),
        cursors: Mutex::default(),
        next: AtomicU64::new(1),
        out,
        in_flight: Mutex::default(),
    });
    while let Some(line) = lines.recv().await {
        let request: Request = match serde_json::from_slice(&line) {
            Ok(request) => request,
            Err(error) => {
                let _ = server.out.send(Response::err(
                    0,
                    RpcError::new(code::PARSE_ERROR, error.to_string()),
                ));
                continue;
            }
        };
        // Taken here, in order: the request it names was registered before it.
        if request.method == method::CANCEL_REQUEST {
            if let Ok(cancel) = parse::<CancelRequest>(request.params)
                && let Some(work) = lock(&server.in_flight).remove(&cancel.id)
            {
                work.abort();
            }
            continue;
        }
        let id = request.id;
        let (abort, registration) = AbortHandle::new_pair();
        lock(&server.in_flight).insert(id, abort);
        let server = server.clone();
        tokio::spawn(async move {
            let work = Abortable::new(server.handle(request), registration);
            if let Ok(response) = work.await {
                let _ = server.out.send(response);
            }
            lock(&server.in_flight).remove(&id);
        });
    }
    server.close_all().await;
}

/// The host's lines until its end, or until one too long, which ends the conversation.
fn read_lines(mut reader: impl BufRead, lines: tokio::sync::mpsc::Sender<Vec<u8>>) {
    let mut line = Vec::new();
    while let Ok(true) = protocol::read_line(&mut reader, &mut line) {
        if !line.is_empty() && lines.blocking_send(std::mem::take(&mut line)).is_err() {
            return;
        }
    }
}

fn write_lines(mut writer: impl Write, responses: std::sync::mpsc::Receiver<Response>) {
    for response in responses {
        if protocol::write_line(&mut writer, &response).is_err() {
            return;
        }
    }
}

/// A result being read, and the events read past the last page.
struct Open {
    stream: QueryStream,
    pending: VecDeque<QueryEvent>,
}

type Sessions = HashMap<String, Arc<Box<dyn Session>>>;
/// By cursor: the session it belongs to, and the result.
type Cursors = HashMap<String, (String, Arc<tokio::sync::Mutex<Open>>)>;

pub(crate) struct Server {
    factories: Vec<Arc<dyn ConnectionFactory>>,
    version: String,
    sessions: Mutex<Sessions>,
    cursors: Mutex<Cursors>,
    next: AtomicU64,
    out: std::sync::mpsc::Sender<Response>,
    /// The requests being answered, by id: what `$/cancel_request` drops.
    in_flight: Mutex<HashMap<u64, AbortHandle>>,
}

impl Server {
    async fn handle(&self, request: Request) -> Response {
        let id = request.id;
        match self.dispatch(request).await {
            Ok(result) => Response::ok(id, result),
            Err(error) => Response::err(id, error),
        }
    }

    async fn dispatch(&self, request: Request) -> Result<Value, RpcError> {
        let params = request.params;
        match request.method.as_str() {
            method::INITIALIZE => reply(self.initialize(parse(params)?)?),
            method::SESSION_OPEN => reply(self.open(parse(params)?).await?),
            method::SESSION_CLOSE => {
                let call: OnSession<()> = parse(params)?;
                reply(self.close(&call.session).await?)
            }
            method::QUERY_EXECUTE => reply(self.execute(parse(params)?).await?),
            method::QUERY_FETCH => {
                let call: Cursor = parse(params)?;
                reply(self.fetch(&call.cursor).await?)
            }
            method::QUERY_CLOSE => {
                let call: Cursor = parse(params)?;
                lock(&self.cursors).remove(&call.cursor);
                reply(())
            }
            method::QUERY_CANCEL => {
                let call: OnSession<uuid::Uuid> = parse(params)?;
                let session = self.session(&call.session)?;
                session
                    .cancel(QueryId(call.args))
                    .await
                    .map_err(|error| RpcError::driver(&error))?;
                reply(())
            }
            other => families::dispatch(self, other, params).await,
        }
    }

    fn initialize(&self, params: InitializeParams) -> Result<InitializeResult, RpcError> {
        if !params.protocols.contains(&protocol::PROTOCOL) {
            return Err(RpcError::new(
                code::INVALID_REQUEST,
                format!(
                    "this plugin speaks protocol {}, and Dexo {} offered {:?}",
                    protocol::PROTOCOL,
                    params.dexo_version,
                    params.protocols
                ),
            ));
        }
        Ok(InitializeResult {
            protocol: protocol::PROTOCOL,
            driver_version: self.version.clone(),
            drivers: self
                .factories
                .iter()
                .map(|factory| factory.driver_name())
                .collect(),
        })
    }

    async fn open(&self, params: SessionOpenParams) -> Result<SessionOpenResult, RpcError> {
        let factory = self
            .factories
            .iter()
            .find(|factory| factory.driver_name() == params.driver)
            .cloned()
            .ok_or_else(|| {
                RpcError::new(
                    code::INVALID_PARAMS,
                    format!("this plugin has no driver '{}'", params.driver),
                )
            })?;
        let session = factory
            .connect(params.into_request())
            .await
            .map_err(|error| RpcError::driver(&error))?;
        let id = format!("s{}", self.next.fetch_add(1, Ordering::Relaxed));
        let opened = SessionOpenResult {
            session: id.clone(),
            capabilities: session.capabilities().to_vec(),
            families: families::of(&*session),
            server_session_id: session.server_session_id(),
            transaction: session.transactions().map(|control| control.state()),
        };
        lock(&self.sessions).insert(id, Arc::new(session));
        Ok(opened)
    }

    pub(crate) fn session(&self, id: &str) -> Result<Arc<Box<dyn Session>>, RpcError> {
        lock(&self.sessions)
            .get(id)
            .cloned()
            .ok_or_else(|| RpcError::new(code::UNKNOWN_SESSION, format!("no session '{id}'")))
    }

    async fn close(&self, id: &str) -> Result<(), RpcError> {
        lock(&self.cursors).retain(|_, (session, _)| session != id);
        let session = lock(&self.sessions)
            .remove(id)
            .ok_or_else(|| RpcError::new(code::UNKNOWN_SESSION, format!("no session '{id}'")))?;
        // A request still running holds the session: it is dropped when that one ends.
        match Arc::try_unwrap(session) {
            Ok(session) => session
                .close()
                .await
                .map_err(|error| RpcError::driver(&error)),
            Err(_) => Ok(()),
        }
    }

    async fn close_all(&self) {
        lock(&self.cursors).clear();
        let sessions: Vec<_> = lock(&self.sessions)
            .drain()
            .map(|(_, session)| session)
            .collect();
        for session in sessions {
            if let Ok(session) = Arc::try_unwrap(session) {
                let _ = session.close().await;
            }
        }
    }

    async fn execute(&self, call: OnSession<QueryExecute>) -> Result<QueryPage, RpcError> {
        let session = self.session(&call.session)?;
        let stream = session
            .execute(call.args.into_request())
            .await
            .map_err(|error| RpcError::driver(&error))?;
        // Answered as soon as the driver has the query, as a built-in driver's `execute`
        // returns: a cancel sent next reaches it, running or queued. Its events come with
        // `query/fetch`.
        let open = Open {
            stream,
            pending: VecDeque::new(),
        };
        let cursor = format!("c{}", self.next.fetch_add(1, Ordering::Relaxed));
        lock(&self.cursors).insert(
            cursor.clone(),
            (call.session, Arc::new(tokio::sync::Mutex::new(open))),
        );
        Ok(QueryPage {
            cursor: Some(cursor),
            events: Vec::new(),
            done: false,
            error: None,
            transaction: None,
        })
    }

    async fn fetch(&self, cursor: &str) -> Result<QueryPage, RpcError> {
        let (session, open) = lock(&self.cursors)
            .get(cursor)
            .cloned()
            .ok_or_else(|| RpcError::new(code::UNKNOWN_CURSOR, format!("no cursor '{cursor}'")))?;
        let mut page = next_page(&mut *open.lock().await).await;
        if page.done {
            lock(&self.cursors).remove(cursor);
            page.transaction = self
                .session(&session)
                .ok()
                .and_then(|session| session.transactions().map(|control| control.state()));
        } else {
            page.cursor = Some(cursor.to_string());
        }
        Ok(page)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn parse<T: DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params)
        .map_err(|error| RpcError::new(code::INVALID_PARAMS, error.to_string()))
}

pub(crate) fn reply(result: impl Serialize) -> Result<Value, RpcError> {
    serde_json::to_value(result).map_err(|error| RpcError::new(code::INTERNAL, error.to_string()))
}

/// Events up to a page's worth -- [`protocol::PAGE_ROWS`] rows, as many other events, or
/// about [`protocol::PAGE_BYTES`] of values -- a batch larger than that split across pages.
/// The stream is read only here, so the driver works only as fast as the host reads: a
/// script of writes, which has no rows, comes a page of statements at a time too.
async fn next_page(open: &mut Open) -> QueryPage {
    let mut events = Vec::new();
    let (mut rows, mut others, mut bytes) = (0usize, 0usize, 0usize);
    let page = |events, done, error| QueryPage {
        cursor: None,
        events,
        done,
        error,
        transaction: None,
    };
    loop {
        if rows >= protocol::PAGE_ROWS
            || others >= protocol::PAGE_ROWS
            || bytes >= protocol::PAGE_BYTES
        {
            return page(events, false, None);
        }
        let event = match open.pending.pop_front() {
            Some(event) => event,
            None => match open.stream.next().await {
                Some(Ok(event)) => event,
                Some(Err(error)) => return page(events, true, Some(error)),
                None => return page(events, true, None),
            },
        };
        let QueryEvent::Rows(batch) = event else {
            others += 1;
            events.push(event);
            continue;
        };
        let room = protocol::PAGE_ROWS - rows;
        let mut taken = Vec::new();
        let mut rest = batch.rows.into_iter();
        for row in rest.by_ref() {
            bytes += row.iter().map(weight).sum::<usize>();
            taken.push(row);
            if taken.len() == room || bytes >= protocol::PAGE_BYTES {
                break;
            }
        }
        rows += taken.len();
        let rest: Vec<_> = rest.collect();
        if !rest.is_empty() {
            open.pending
                .push_front(QueryEvent::Rows(RowBatch { rows: rest }));
        }
        events.push(QueryEvent::Rows(RowBatch { rows: taken }));
    }
}

/// About how many bytes a value takes on the wire.
fn weight(value: &DbValue) -> usize {
    match value {
        DbValue::Null | DbValue::Bool(_) => 4,
        DbValue::I64(_) | DbValue::U64(_) => 20,
        DbValue::Decimal(text) | DbValue::Text(text) | DbValue::Json(text) => text.len() + 16,
        DbValue::Bytes(bytes) => bytes.len() * 4 / 3 + 16,
        DbValue::Native {
            type_name,
            bytes,
            text,
        } => type_name.len() + bytes.len() * 4 / 3 + text.len() + 32,
    }
}
