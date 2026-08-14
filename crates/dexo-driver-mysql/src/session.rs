use std::sync::Arc;

use dexo_driver_api::{
    ColumnMeta, DriverError, QueryEvent, QueryId, QueryRequest, QueryStream, RowBatch, Session,
    TransactionControl, TransactionMode, TransactionState, validate_savepoint,
};
use futures_util::StreamExt;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts};
use tokio::sync::Mutex;

use crate::decode::{column_meta, decode_row};
use crate::error::map_error;
use crate::factory::capabilities;

pub const ROW_BATCH_SIZE: usize = 256;

pub struct MysqlSession {
    pub(crate) conn: Arc<Mutex<Conn>>,
    conn_id: u32,
    opts: Opts,
    capabilities: Vec<dexo_driver_api::CapabilityState>,
    tx_state: std::sync::Mutex<TransactionState>,
}

impl MysqlSession {
    pub(crate) fn new(conn: Arc<Mutex<Conn>>, opts: Opts, conn_id: u32) -> Self {
        Self {
            conn,
            conn_id,
            opts,
            capabilities: capabilities(),
            tx_state: std::sync::Mutex::new(TransactionState::Idle),
        }
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
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            let mut conn = conn.lock().await;
            // ponytail: text protocol; CREATE FUNCTION/EVENT cannot be prepared (MySQL 1295). Bind params when execute grows a param path.
            let result = match conn.query_iter(sql).await {
                Ok(result) => result,
                Err(error) => {
                    let _ = tx.send(Err(map_error(error))).await;
                    return;
                }
            };
            let columns: Vec<ColumnMeta> = result.columns_ref().iter().map(column_meta).collect();
            if tx.send(Ok(QueryEvent::Columns(columns))).await.is_err() {
                return;
            }
            let mut stream = match result.stream_and_drop::<mysql_async::Row>().await {
                Ok(Some(stream)) => stream,
                Ok(None) => {
                    let _ = tx
                        .send(Ok(QueryEvent::Finished {
                            rows_affected: Some(0),
                        }))
                        .await;
                    return;
                }
                Err(error) => {
                    let _ = tx.send(Err(map_error(error))).await;
                    return;
                }
            };
            let mut batch = Vec::new();
            let mut emitted = 0_u64;
            while let Some(row) = stream.next().await {
                let row = match row {
                    Ok(row) => row,
                    Err(error) => {
                        let _ = tx.send(Err(map_error(error))).await;
                        return;
                    }
                };
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
            if !batch.is_empty()
                && tx
                    .send(Ok(QueryEvent::Rows(RowBatch { rows: batch })))
                    .await
                    .is_err()
            {
                return;
            }
            let _ = tx
                .send(Ok(QueryEvent::Finished {
                    rows_affected: None,
                }))
                .await;
        });
        Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async {
            rx.recv().await.map(|item| (item, rx))
        })))
    }

    async fn cancel(&self, _query: QueryId) -> Result<(), DriverError> {
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
