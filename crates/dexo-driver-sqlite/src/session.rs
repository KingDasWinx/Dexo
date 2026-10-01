use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use dexo_driver_api::{
    CapabilityState, ColumnMeta, DbValue, DriverError, DriverErrorCategory, QueryEvent, QueryId,
    QueryRequest, QueryStream, RowBatch, Session, TransactionControl, TransactionMode,
    TransactionState, validate_savepoint,
};
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::{Connection, InterruptHandle, Statement};
use tokio::sync::mpsc::Sender;

use crate::decode::{column_meta, decode, to_sql};
use crate::error::{internal, map_error};
use crate::factory::capabilities;

pub const ROW_BATCH_SIZE: usize = 256;

type Events = Sender<Result<QueryEvent, DriverError>>;

/// One SQLite connection. rusqlite is blocking, so every call runs on a blocking thread
/// with the connection locked; the interrupt handle lives outside the lock, so a cancel
/// reaches a statement that is holding it.
pub struct SqliteSession {
    conn: Arc<Mutex<Connection>>,
    interrupt: Arc<InterruptHandle>,
    read_only: bool,
    capabilities: Vec<CapabilityState>,
    tx_state: Mutex<TransactionState>,
    /// `begin(ReadOnly)` turned `query_only` on, so ending the transaction turns it off.
    query_only: AtomicBool,
}

impl SqliteSession {
    pub(crate) fn new(conn: Connection, read_only: bool) -> Self {
        let interrupt = Arc::new(conn.get_interrupt_handle());
        Self {
            conn: Arc::new(Mutex::new(conn)),
            interrupt,
            read_only,
            capabilities: capabilities(),
            tx_state: Mutex::new(TransactionState::Idle),
            query_only: AtomicBool::new(false),
        }
    }

    /// Runs `work` on a blocking thread with the connection to itself. A panic in an
    /// earlier call leaves the connection as usable as SQLite left it, so a poisoned lock
    /// is taken anyway.
    pub(crate) async fn with_conn<T, F>(&self, work: F) -> Result<T, DriverError>
    where
        F: FnOnce(&mut Connection) -> Result<T, DriverError> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock().unwrap_or_else(PoisonError::into_inner);
            work(&mut conn)
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

    /// Ends the transaction, and only then lifts the `query_only` a read-only one set: a
    /// rollback that failed leaves the transaction open, and it stays read-only.
    async fn end(&self, sql: &'static str, failed: TransactionState) -> Result<(), DriverError> {
        let lift = self.query_only.load(Ordering::SeqCst);
        let result = self
            .with_conn(move |conn| {
                conn.execute_batch(sql).map_err(map_error)?;
                if lift {
                    conn.execute_batch("PRAGMA query_only = OFF")
                        .map_err(map_error)?;
                }
                Ok(())
            })
            .await;
        if result.is_ok() {
            self.query_only.store(false, Ordering::SeqCst);
        }
        self.set_state(if result.is_ok() {
            TransactionState::Idle
        } else {
            failed
        });
        result
    }
}

#[async_trait::async_trait]
impl Session for SqliteSession {
    fn capabilities(&self) -> &[CapabilityState] {
        &self.capabilities
    }

    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError> {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let conn = Arc::clone(&self.conn);
        let interrupt = Arc::clone(&self.interrupt);
        let read_only = self.read_only;
        let QueryRequest {
            sql,
            parameters,
            row_limit,
            timeout,
            ..
        } = request;
        let events = tx.clone();
        tokio::spawn(async move {
            let mut run = tokio::task::spawn_blocking(move || {
                let conn = conn.lock().unwrap_or_else(PoisonError::into_inner);
                if let Err(error) =
                    run_statements(&conn, &sql, &parameters, row_limit, read_only, &events)
                {
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
                interrupt.interrupt();
            }
        });
        Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async {
            rx.recv().await.map(|item| (item, rx))
        })))
    }

    /// Interrupts whatever statement the connection is running; a connection runs one
    /// at a time, so that is the query asked about or none.
    async fn cancel(&self, _query: QueryId) -> Result<(), DriverError> {
        self.interrupt.interrupt();
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

/// Each statement of `sql` in turn, as its own result set. Returns early, without an
/// error, when the receiver has gone.
fn run_statements(
    conn: &Connection,
    sql: &str,
    parameters: &[DbValue],
    row_limit: u64,
    read_only: bool,
    events: &Events,
) -> Result<(), DriverError> {
    let send = |event| events.blocking_send(Ok(event)).is_ok();
    let mut batch = rusqlite::Batch::new(conn, sql);
    let mut index = 0;
    let mut last_affected = None;
    while let Some(mut statement) = batch.next().map_err(map_error)? {
        let writes = !statement.readonly() && statement.is_explain() == 0;
        // The file is opened read-only, so SQLite would refuse the write itself; this
        // says so before a VACUUM INTO writes a copy somewhere else.
        if read_only && writes {
            return Err(DriverError::new(
                DriverErrorCategory::Permission,
                "the connection is read-only, and this statement writes",
            ));
        }
        bind(&mut statement, parameters)?;
        let columns: Vec<ColumnMeta> = statement.columns().iter().map(column_meta).collect();
        let width = columns.len();
        let before = conn.total_changes();
        if !send(QueryEvent::ResultSetStarted { index }) || !send(QueryEvent::Columns(columns)) {
            return Ok(());
        }
        let mut rows = statement.raw_query();
        let mut batch_rows = Vec::new();
        let mut emitted = 0_u64;
        while row_limit == 0 || emitted < row_limit {
            let Some(row) = rows.next().map_err(map_error)? else {
                break;
            };
            batch_rows.push((0..width).map(|i| decode(row.get_ref_unwrap(i))).collect());
            emitted += 1;
            if batch_rows.len() >= ROW_BATCH_SIZE
                && !send(QueryEvent::Rows(RowBatch {
                    rows: std::mem::take(&mut batch_rows),
                }))
            {
                return Ok(());
            }
        }
        drop(rows);
        if !batch_rows.is_empty() && !send(QueryEvent::Rows(RowBatch { rows: batch_rows })) {
            return Ok(());
        }
        // `changes()` keeps the count of the last statement that changed rows, so a
        // CREATE after an INSERT would report the INSERT's rows as its own.
        let rows_affected = writes.then(|| {
            if conn.total_changes() == before {
                0
            } else {
                conn.changes()
            }
        });
        last_affected = rows_affected;
        if !send(QueryEvent::ResultSetFinished {
            index,
            rows_affected,
        }) {
            return Ok(());
        }
        index += 1;
    }
    send(QueryEvent::Finished {
        rows_affected: last_affected,
    });
    Ok(())
}

/// Binds by position: `?`, `?NNN`, and `:name` in the order the names first appear,
/// which is the order the editor collects their values in.
fn bind(statement: &mut Statement<'_>, parameters: &[DbValue]) -> Result<(), DriverError> {
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
    for (index, value) in parameters.iter().enumerate() {
        statement
            .raw_bind_parameter(index + 1, to_sql(value))
            .map_err(map_error)?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl TransactionControl for SqliteSession {
    /// SQLite has no read-only transaction; `query_only` makes the connection refuse
    /// writes until the transaction ends.
    async fn begin(&self, mode: TransactionMode) -> Result<(), DriverError> {
        match mode {
            TransactionMode::ReadWrite => self.exec_batch("BEGIN").await?,
            TransactionMode::ReadOnly => {
                self.exec_batch("BEGIN; PRAGMA query_only = ON").await?;
                self.query_only.store(true, Ordering::SeqCst);
            }
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

    async fn savepoint(&self, name: &str) -> Result<(), DriverError> {
        validate_savepoint(name)?;
        let sql = format!("SAVEPOINT {name}");
        self.with_conn(move |conn| conn.execute_batch(&sql).map_err(map_error))
            .await
    }

    async fn rollback_to(&self, name: &str) -> Result<(), DriverError> {
        validate_savepoint(name)?;
        let sql = format!("ROLLBACK TO SAVEPOINT {name}");
        self.with_conn(move |conn| conn.execute_batch(&sql).map_err(map_error))
            .await
    }

    async fn release_savepoint(&self, name: &str) -> Result<(), DriverError> {
        validate_savepoint(name)?;
        let sql = format!("RELEASE SAVEPOINT {name}");
        self.with_conn(move |conn| conn.execute_batch(&sql).map_err(map_error))
            .await
    }

    fn state(&self) -> TransactionState {
        *self.tx_state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
