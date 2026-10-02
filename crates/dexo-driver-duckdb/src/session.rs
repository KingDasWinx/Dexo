use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use dexo_driver_api::{
    CapabilityState, ColumnMeta, DbValue, DriverError, DriverErrorCategory, QueryEvent, QueryId,
    QueryRequest, QueryStream, RowBatch, Session, TransactionControl, TransactionMode,
    TransactionState,
};
use dexo_sql::Dialect;
use duckdb::arrow::array::{Array, ArrayRef};
use duckdb::arrow::datatypes::{DataType, Schema};
use duckdb::{Connection, InterruptHandle};
use tokio::sync::mpsc::Sender;

use crate::decode::{column_meta, decode_column, to_sql};
use crate::error::{internal, map_error, writes_refused};
use crate::factory::capabilities;

pub const ROW_BATCH_SIZE: usize = 256;

type Events = Sender<Result<QueryEvent, DriverError>>;

/// One DuckDB connection. duckdb-rs is blocking, so every call runs on a blocking thread
/// with the connection locked; the interrupt handle lives outside the lock, so a cancel
/// reaches a statement that is holding it.
pub struct DuckdbSession {
    conn: Arc<Mutex<Connection>>,
    interrupt: Arc<InterruptHandle>,
    /// Which query holds the connection, and one cancelled before it got there. The
    /// interrupt reaches whatever is running -- a catalog load the query waits behind,
    /// say -- so it is only sent when that is the query being cancelled.
    live: Arc<Mutex<Live>>,
    read_only: bool,
    capabilities: Vec<CapabilityState>,
    tx_state: Mutex<TransactionState>,
}

impl DuckdbSession {
    pub(crate) fn new(conn: Connection, read_only: bool) -> Self {
        let interrupt = conn.interrupt_handle();
        Self {
            conn: Arc::new(Mutex::new(conn)),
            interrupt,
            live: Arc::new(Mutex::new(Live::default())),
            read_only,
            capabilities: capabilities(),
            tx_state: Mutex::new(TransactionState::Idle),
        }
    }

    pub(crate) fn read_only(&self) -> bool {
        self.read_only
    }

    /// Runs `work` on a blocking thread with the connection to itself. A panic in an
    /// earlier call leaves the connection as usable as DuckDB left it, so a poisoned lock
    /// is taken anyway.
    pub(crate) async fn with_conn<T, F>(&self, work: F) -> Result<T, DriverError>
    where
        F: FnOnce(&Connection) -> Result<T, DriverError> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(PoisonError::into_inner);
            work(&conn)
        })
        .await
        .map_err(internal)?
    }

    async fn exec_batch(&self, sql: &'static str) -> Result<(), DriverError> {
        self.with_conn(move |conn| conn.execute_batch(sql).map_err(map_error))
            .await
    }

    fn set_state(&self, state: TransactionState) {
        *self.tx_state.lock().unwrap_or_else(PoisonError::into_inner) = state;
    }

    async fn end(&self, sql: &'static str, failed: TransactionState) -> Result<(), DriverError> {
        let result = self.exec_batch(sql).await;
        self.set_state(if result.is_ok() {
            TransactionState::Idle
        } else {
            failed
        });
        result
    }
}

#[derive(Default)]
struct Live {
    running: Option<QueryId>,
    cancelled: Option<QueryId>,
}

/// Stops `query`: interrupts it if it is the statement running, or marks it so it
/// never starts if it is still waiting for the connection.
fn stop(live: &Mutex<Live>, interrupt: &InterruptHandle, query: QueryId) {
    let mut live = live.lock().unwrap_or_else(PoisonError::into_inner);
    if live.running == Some(query) {
        interrupt.interrupt();
    } else {
        live.cancelled = Some(query);
    }
}

/// The statements of `sql`, split as the editor splits them: DuckDB's own prepare runs
/// every statement but the last as it parses them, before Dexo could look at one.
pub(crate) fn statements(sql: &str) -> Vec<&str> {
    dexo_sql::split_statements_in(sql, Dialect::Duckdb)
        .into_iter()
        .map(|span| sql[span.byte_range].trim())
        .filter(|text| !text.trim_end_matches(';').trim().is_empty())
        .collect()
}

pub(crate) fn is_read(sql: &str) -> bool {
    dexo_sql::is_read(sql, Dialect::Duckdb)
}

/// Opens a transaction for Dexo's own use, or says the user already has one open:
/// DuckDB has no savepoint to nest one inside theirs.
pub(crate) fn begin_own(conn: &Connection, sql: &str) -> Result<bool, DriverError> {
    match conn.execute_batch(sql) {
        Ok(()) => Ok(true),
        Err(error) if error.to_string().contains("within a transaction") => Ok(false),
        Err(error) => Err(map_error(error)),
    }
}

#[async_trait::async_trait]
impl Session for DuckdbSession {
    fn capabilities(&self) -> &[CapabilityState] {
        &self.capabilities
    }

    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError> {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let conn = Arc::clone(&self.conn);
        let interrupt = Arc::clone(&self.interrupt);
        let live = Arc::clone(&self.live);
        let read_only = self.read_only;
        let QueryRequest {
            id,
            sql,
            parameters,
            row_limit,
            timeout,
            read_only: reads_only,
            ..
        } = request;
        let events = tx.clone();
        let running = Arc::clone(&live);
        tokio::spawn(async move {
            let mut run = tokio::task::spawn_blocking(move || {
                let conn = conn.lock().unwrap_or_else(PoisonError::into_inner);
                {
                    let mut live = running.lock().unwrap_or_else(PoisonError::into_inner);
                    if live.cancelled == Some(id) {
                        live.cancelled = None;
                        drop(live);
                        let _ = events.blocking_send(Err(DriverError::new(
                            DriverErrorCategory::Cancelled,
                            "query cancelled",
                        )));
                        return;
                    }
                    live.running = Some(id);
                }
                let outcome = run_script(
                    &conn,
                    &sql,
                    &parameters,
                    row_limit,
                    Guard {
                        read_only,
                        reads_only,
                    },
                    &events,
                );
                running
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .running = None;
                if let Err(error) = outcome {
                    let _ = events.blocking_send(Err(error));
                }
            });
            if timeout == Duration::ZERO {
                let _ = run.await;
                return;
            }
            if tokio::time::timeout(timeout, &mut run).await.is_err() {
                let _ = tx
                    .send(Err(DriverError::new(
                        DriverErrorCategory::Timeout,
                        "query timed out",
                    )))
                    .await;
                stop(&live, &interrupt, id);
            }
        });
        Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async {
            rx.recv().await.map(|item| (item, rx))
        })))
    }

    /// Interrupts the query asked about if it is the one running, or stops it from
    /// starting if it is still queued; never whatever else holds the connection.
    async fn cancel(&self, query: QueryId) -> Result<(), DriverError> {
        stop(&self.live, &self.interrupt, query);
        Ok(())
    }

    async fn close(self: Box<Self>) -> Result<(), DriverError> {
        Ok(())
    }

    fn transactions(&self) -> Option<&dyn TransactionControl> {
        Some(self)
    }

    fn catalog(&self) -> Option<&dyn dexo_driver_api::CatalogReader> {
        Some(self)
    }

    fn data(&self) -> Option<&dyn dexo_driver_api::DataMutator> {
        Some(self)
    }

    fn bulk(&self) -> Option<&dyn dexo_driver_api::BulkWriter> {
        Some(self)
    }

    fn explain(&self) -> Option<&dyn dexo_driver_api::ExplainProvider> {
        Some(self)
    }
}

#[derive(Clone, Copy)]
struct Guard {
    /// A read-only profile: the file is open read-only, and a statement that would write
    /// anywhere else -- `COPY ... TO` a file, `ATTACH` another database -- is refused.
    read_only: bool,
    /// Text asked to only read, around Dexo's own.
    reads_only: bool,
}

/// Each statement of `sql` in turn, as its own result set. Where it may only read, every
/// statement has to read; asked to only read, outside a transaction it also runs in a
/// read-only one rolled back after it. Inside the user's, DuckDB has no savepoint to
/// fence it with, and the check is all there is.
fn run_script(
    conn: &Connection,
    sql: &str,
    parameters: &[DbValue],
    row_limit: u64,
    guard: Guard,
    events: &Events,
) -> Result<(), DriverError> {
    let statements = statements(sql);
    if (guard.read_only || guard.reads_only) && !statements.iter().all(|text| is_read(text)) {
        return Err(writes_refused());
    }
    let fenced = guard.reads_only && begin_own(conn, "BEGIN TRANSACTION READ ONLY")?;
    let outcome = run_statements(conn, &statements, parameters, row_limit, events);
    if fenced {
        let rolled_back = conn.execute_batch("ROLLBACK").map_err(map_error);
        outcome?;
        rolled_back?;
        return Ok(());
    }
    outcome
}

/// Returns early, without an error, when the receiver has gone.
fn run_statements(
    conn: &Connection,
    statements: &[&str],
    parameters: &[DbValue],
    row_limit: u64,
    events: &Events,
) -> Result<(), DriverError> {
    let send = |event| events.blocking_send(Ok(event)).is_ok();
    let mut last_affected = None;
    for (index, text) in statements.iter().enumerate() {
        let mut statement = conn.prepare(text).map_err(map_error)?;
        let expected = statement.parameter_count();
        if parameters.len() != expected {
            return Err(DriverError::new(
                DriverErrorCategory::Syntax,
                format!(
                    "expected {expected} query parameters, received {}",
                    parameters.len()
                ),
            ));
        }
        let values = parameters.iter().map(to_sql);
        // The stream borrows the statement; its chunks are read with `step`, which
        // reports a failure where the iterator would panic.
        let _ = statement
            .stream_arrow(duckdb::params_from_iter(values))
            .map_err(map_error)?;
        let schema = statement.schema();
        if !send(QueryEvent::ResultSetStarted { index }) {
            return Ok(());
        }
        if let Some(status) = Status::of(&schema).filter(|_| !is_read(text)) {
            let mut affected = 0;
            while let Some(chunk) = statement.step().map_err(map_error)? {
                affected += status.count(chunk.columns().first());
            }
            let rows_affected = matches!(status, Status::Count).then_some(affected);
            last_affected = rows_affected;
            if !send(QueryEvent::Columns(Vec::new()))
                || !send(QueryEvent::ResultSetFinished {
                    index,
                    rows_affected,
                    truncated: false,
                })
            {
                return Ok(());
            }
            continue;
        }
        let columns: Vec<ColumnMeta> = schema
            .fields()
            .iter()
            .enumerate()
            .map(|(column, field)| {
                column_meta(field.name(), &statement.column_logical_type(column))
            })
            .collect();
        let type_names: Vec<String> = columns
            .iter()
            .map(|column| column.type_name.clone())
            .collect();
        if !send(QueryEvent::Columns(columns)) {
            return Ok(());
        }
        let mut emitted = 0_u64;
        let mut truncated = false;
        let mut batch_rows = Vec::new();
        'chunks: while let Some(chunk) = statement.step().map_err(map_error)? {
            let rows = decode_rows(chunk.columns(), &type_names)?;
            for row in rows {
                if row_limit > 0 && emitted == row_limit {
                    truncated = true;
                    break 'chunks;
                }
                batch_rows.push(row);
                emitted += 1;
                if batch_rows.len() >= ROW_BATCH_SIZE
                    && !send(QueryEvent::Rows(RowBatch {
                        rows: std::mem::take(&mut batch_rows),
                    }))
                {
                    return Ok(());
                }
            }
        }
        drop(statement);
        if !batch_rows.is_empty() && !send(QueryEvent::Rows(RowBatch { rows: batch_rows })) {
            return Ok(());
        }
        last_affected = None;
        if !send(QueryEvent::ResultSetFinished {
            index,
            rows_affected: None,
            truncated,
        }) {
            return Ok(());
        }
    }
    send(QueryEvent::Finished {
        rows_affected: last_affected,
    });
    Ok(())
}

/// The rows of one chunk, its columns decoded together.
pub(crate) fn decode_rows(
    arrays: &[ArrayRef],
    type_names: &[String],
) -> Result<Vec<Vec<DbValue>>, DriverError> {
    let len = arrays.first().map_or(0, |array| array.len());
    let mut rows: Vec<Vec<DbValue>> = (0..len).map(|_| Vec::with_capacity(arrays.len())).collect();
    for (array, type_name) in arrays.iter().zip(type_names) {
        for (row, cell) in rows.iter_mut().zip(decode_column(array, type_name)?) {
            row.push(cell);
        }
    }
    Ok(rows)
}

/// What DuckDB answers a statement that returns no rows of its own with: a `Count` of
/// the rows an INSERT, UPDATE or DELETE changed, or a `Success` flag for the rest. A
/// statement that reads -- `SELECT count(*) AS "Count"` -- keeps its column.
#[derive(Clone, Copy)]
enum Status {
    Count,
    Success,
}

impl Status {
    fn of(schema: &Schema) -> Option<Self> {
        let [field] = schema.fields().iter().collect::<Vec<_>>()[..] else {
            return None;
        };
        match (field.name().as_str(), field.data_type()) {
            ("Count", DataType::Int64) => Some(Self::Count),
            ("Success", DataType::Boolean) => Some(Self::Success),
            _ => None,
        }
    }

    fn count(self, column: Option<&ArrayRef>) -> u64 {
        let Some(counts) = column.filter(|_| matches!(self, Self::Count)) else {
            return 0;
        };
        let counts = counts
            .as_any()
            .downcast_ref::<duckdb::arrow::array::Int64Array>();
        counts.map_or(0, |counts| {
            counts
                .iter()
                .flatten()
                .map(|count| count.max(0) as u64)
                .sum()
        })
    }
}

#[async_trait::async_trait]
impl TransactionControl for DuckdbSession {
    async fn begin(&self, mode: TransactionMode) -> Result<(), DriverError> {
        match mode {
            TransactionMode::ReadWrite => self.exec_batch("BEGIN TRANSACTION").await?,
            TransactionMode::ReadOnly => self.exec_batch("BEGIN TRANSACTION READ ONLY").await?,
        }
        self.set_state(TransactionState::Active);
        Ok(())
    }

    async fn commit(&self) -> Result<(), DriverError> {
        self.end("COMMIT", TransactionState::Failed).await
    }

    async fn rollback(&self) -> Result<(), DriverError> {
        self.end("ROLLBACK", TransactionState::Unknown).await
    }

    async fn savepoint(&self, _name: &str) -> Result<(), DriverError> {
        Err(no_savepoints())
    }

    async fn rollback_to(&self, _name: &str) -> Result<(), DriverError> {
        Err(no_savepoints())
    }

    async fn release_savepoint(&self, _name: &str) -> Result<(), DriverError> {
        Err(no_savepoints())
    }

    fn state(&self) -> TransactionState {
        *self.tx_state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn no_savepoints() -> DriverError {
    DriverError::unsupported("DuckDB has no savepoints")
}
