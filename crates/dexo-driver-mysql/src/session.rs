use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dexo_driver_api::{
    ColumnMeta, DbValue, DriverError, DriverErrorCategory, QueryEvent, QueryId, QueryRequest,
    QueryStream, RowBatch, Session, TransactionControl, TransactionMode, TransactionState,
    validate_savepoint,
};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, Params, Value};
use tokio::sync::Mutex;

use crate::decode::{column_meta, decode_row};
use crate::error::map_error;
use crate::factory::capabilities;

pub const ROW_BATCH_SIZE: usize = 256;

pub struct MysqlSession {
    pub(crate) conn: Arc<Mutex<Conn>>,
    conn_id: u32,
    connect_generation: u64,
    live_generation: Arc<AtomicU64>,
    opts: Opts,
    capabilities: Vec<dexo_driver_api::CapabilityState>,
    tx_state: std::sync::Mutex<TransactionState>,
    _lease: Option<dexo_transport::TransportLease>,
}

impl MysqlSession {
    pub(crate) fn new(
        conn: Arc<Mutex<Conn>>,
        opts: Opts,
        conn_id: u32,
        live_generation: Arc<AtomicU64>,
        lease: Option<dexo_transport::TransportLease>,
    ) -> Self {
        let connect_generation = live_generation.load(Ordering::SeqCst);
        Self {
            conn,
            conn_id,
            connect_generation,
            live_generation,
            opts,
            capabilities: capabilities(),
            tx_state: std::sync::Mutex::new(TransactionState::Idle),
            _lease: lease,
        }
    }

    pub fn bump_generation(&self) {
        self.live_generation.fetch_add(1, Ordering::SeqCst);
    }

    fn set_state(&self, state: TransactionState) {
        *self.tx_state.lock().expect("mysql tx state poisoned") = state;
    }
}

#[async_trait::async_trait]
impl Session for MysqlSession {
    fn capabilities(&self) -> &[dexo_driver_api::CapabilityState] {
        &self.capabilities
    }

    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError> {
        let conn = Arc::clone(&self.conn);
        let sql = request.sql;
        let row_limit = request.row_limit;
        let parameters = request.parameters;
        let timeout = request.timeout;
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            let run = run_mysql_query(conn, sql, parameters, row_limit, tx.clone());
            if timeout == Duration::ZERO {
                run.await;
                return;
            }
            if tokio::time::timeout(timeout, run).await.is_err() {
                let _ = tx
                    .send(Err(DriverError::new(
                        DriverErrorCategory::Timeout,
                        "query timed out",
                    )))
                    .await;
            }
        });
        Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async {
            rx.recv().await.map(|item| (item, rx))
        })))
    }

    async fn cancel(&self, _query: QueryId) -> Result<(), DriverError> {
        if self.connect_generation != self.live_generation.load(Ordering::SeqCst) {
            return Err(DriverError::new(
                DriverErrorCategory::Cancelled,
                "session generation changed",
            ));
        }
        // ponytail: cache conn_id at connect so KILL QUERY does not wait on the execute lock.
        // Ceiling: id is stale after a server-side reconnect. Store a generation when sessions reconnect.
        let mut killer = Conn::new(self.opts.clone()).await.map_err(map_error)?;
        killer
            .query_drop(format!("KILL QUERY {}", self.conn_id))
            .await
            .map_err(map_error)
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

    fn ddl(&self) -> Option<&dyn dexo_driver_api::DdlExecutor> {
        Some(self)
    }

    fn security(&self) -> Option<&dyn dexo_driver_api::SecurityAdmin> {
        Some(self)
    }

    fn bulk(&self) -> Option<&dyn dexo_driver_api::BulkWriter> {
        Some(self)
    }

    fn explain(&self) -> Option<&dyn dexo_driver_api::ExplainProvider> {
        Some(self)
    }

    fn admin(&self) -> Option<&dyn dexo_driver_api::AdministrationProvider> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl TransactionControl for MysqlSession {
    async fn begin(&self, mode: TransactionMode) -> Result<(), DriverError> {
        let sql = match mode {
            TransactionMode::ReadWrite => "START TRANSACTION",
            TransactionMode::ReadOnly => "START TRANSACTION READ ONLY",
        };
        self.exec_sql(sql).await?;
        self.set_state(TransactionState::Active);
        Ok(())
    }

    async fn commit(&self) -> Result<(), DriverError> {
        match self.exec_sql("COMMIT").await {
            Ok(()) => {
                self.set_state(TransactionState::Idle);
                Ok(())
            }
            Err(error) => {
                self.set_state(TransactionState::Failed);
                Err(error)
            }
        }
    }

    async fn rollback(&self) -> Result<(), DriverError> {
        match self.exec_sql("ROLLBACK").await {
            Ok(()) => {
                self.set_state(TransactionState::Idle);
                Ok(())
            }
            Err(error) => {
                self.set_state(TransactionState::Unknown);
                Err(error)
            }
        }
    }

    async fn savepoint(&self, name: &str) -> Result<(), DriverError> {
        validate_savepoint(name)?;
        self.exec_sql(&format!("SAVEPOINT {name}")).await
    }

    async fn rollback_to(&self, name: &str) -> Result<(), DriverError> {
        validate_savepoint(name)?;
        self.exec_sql(&format!("ROLLBACK TO SAVEPOINT {name}"))
            .await
    }

    async fn release_savepoint(&self, name: &str) -> Result<(), DriverError> {
        validate_savepoint(name)?;
        self.exec_sql(&format!("RELEASE SAVEPOINT {name}")).await
    }

    fn state(&self) -> TransactionState {
        *self.tx_state.lock().expect("mysql tx state poisoned")
    }
}

impl MysqlSession {
    async fn exec_sql(&self, sql: &str) -> Result<(), DriverError> {
        let mut conn = self.conn.lock().await;
        conn.query_drop(sql).await.map_err(map_error)
    }
}

fn mysql_values(values: &[DbValue]) -> Vec<Value> {
    values
        .iter()
        .map(|value| match value {
            DbValue::Null => Value::NULL,
            DbValue::Bool(value) => Value::Int(i64::from(*value)),
            DbValue::I64(value) => Value::Int(*value),
            DbValue::U64(value) => Value::UInt(*value),
            DbValue::Decimal(text) | DbValue::Text(text) | DbValue::Json(text) => {
                Value::Bytes(text.as_bytes().to_vec())
            }
            DbValue::Bytes(bytes) => Value::Bytes(bytes.clone()),
            DbValue::Native { text, bytes, .. } => {
                if bytes.is_empty() {
                    Value::Bytes(text.as_bytes().to_vec())
                } else {
                    Value::Bytes(bytes.clone())
                }
            }
        })
        .collect()
}

async fn run_mysql_query(
    conn: Arc<Mutex<Conn>>,
    sql: String,
    parameters: Vec<DbValue>,
    row_limit: u64,
    tx: tokio::sync::mpsc::Sender<Result<QueryEvent, DriverError>>,
) {
    let mut conn = conn.lock().await;
    // ponytail: text protocol when unbound; CREATE FUNCTION/EVENT cannot be prepared (MySQL 1295).
    if parameters.is_empty() {
        match conn.query_iter(sql).await {
            Ok(result) => emit_mysql_sets(result, row_limit, tx).await,
            Err(error) => {
                let _ = tx.send(Err(map_error(error))).await;
            }
        }
    } else {
        match conn
            .exec_iter(sql, Params::Positional(mysql_values(&parameters)))
            .await
        {
            Ok(result) => emit_mysql_sets(result, row_limit, tx).await,
            Err(error) => {
                let _ = tx.send(Err(map_error(error))).await;
            }
        }
    }
}

async fn emit_mysql_sets<P>(
    mut result: mysql_async::QueryResult<'_, '_, P>,
    row_limit: u64,
    tx: tokio::sync::mpsc::Sender<Result<QueryEvent, DriverError>>,
) where
    P: mysql_async::prelude::Protocol + Unpin,
{
    let mut index = 0usize;
    let mut last_affected = None;
    loop {
        if result.is_empty() {
            break;
        }
        if tx
            .send(Ok(QueryEvent::ResultSetStarted { index }))
            .await
            .is_err()
        {
            return;
        }
        let columns: Vec<ColumnMeta> = result.columns_ref().iter().map(column_meta).collect();
        if tx.send(Ok(QueryEvent::Columns(columns))).await.is_err() {
            return;
        }
        let mut batch = Vec::new();
        let mut emitted = 0_u64;
        loop {
            match result.next().await {
                Ok(Some(row)) => {
                    batch.push(decode_row(&row));
                    emitted += 1;
                    if batch.len() >= ROW_BATCH_SIZE
                        && tx
                            .send(Ok(QueryEvent::Rows(RowBatch {
                                rows: std::mem::take(&mut batch),
                            })))
                            .await
                            .is_err()
                    {
                        return;
                    }
                    if row_limit > 0 && emitted >= row_limit {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = tx.send(Err(map_error(error))).await;
                    return;
                }
            }
        }
        if !batch.is_empty()
            && tx
                .send(Ok(QueryEvent::Rows(RowBatch { rows: batch })))
                .await
                .is_err()
        {
            return;
        }
        let rows_affected = Some(result.affected_rows());
        last_affected = rows_affected;
        if tx
            .send(Ok(QueryEvent::ResultSetFinished {
                index,
                rows_affected,
            }))
            .await
            .is_err()
        {
            return;
        }
        index += 1;
        if result.is_empty() {
            break;
        }
    }
    let _ = tx
        .send(Ok(QueryEvent::Finished {
            rows_affected: last_affected,
        }))
        .await;
}
