use std::pin::pin;
use std::sync::{Arc, Mutex};

use dexo_driver_api::{
    ColumnMeta, DriverError, QueryEvent, QueryId, QueryRequest, QueryStream, RowBatch, Session,
    TransactionControl, TransactionMode, TransactionState, validate_savepoint,
};
use futures_util::StreamExt;
use tokio_postgres::types::ToSql;

use crate::decode::{column_meta, decode_row};
use crate::error::map_error;
use crate::factory::capabilities;

pub const ROW_BATCH_SIZE: usize = 256;

pub struct PostgresSession {
    pub(crate) client: Arc<tokio_postgres::Client>,
    capabilities: Vec<dexo_driver_api::CapabilityState>,
    tx_state: Mutex<TransactionState>,
}

impl PostgresSession {
    pub(crate) fn new(client: tokio_postgres::Client) -> Self {
        Self {
            client: Arc::new(client),
            capabilities: capabilities(),
            tx_state: Mutex::new(TransactionState::Idle),
        }
    }

    fn set_state(&self, state: TransactionState) {
        *self.tx_state.lock().expect("postgres tx state poisoned") = state;
    }
}

#[async_trait::async_trait]
impl Session for PostgresSession {
    fn capabilities(&self) -> &[dexo_driver_api::CapabilityState] {
        &self.capabilities
    }

    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError> {
        let client = Arc::clone(&self.client);
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let row_limit = request.row_limit;
        let sql = request.sql;
        tokio::spawn(async move {
            // ponytail: prepare+query_raw+consume stay on one task with Client; splitting query_raw
            // across tasks makes PostgreSQL cancel packets miss the waiting RowStream.
            let statement = match client.prepare(&sql).await {
                Ok(statement) => statement,
                Err(error) => {
                    let _ = tx.send(Err(map_error(error))).await;
                    return;
                }
            };
            let columns: Vec<ColumnMeta> = statement.columns().iter().map(column_meta).collect();
            let rows = match client
                .query_raw(&statement, Vec::<&(dyn ToSql + Sync)>::new())
                .await
            {
                Ok(rows) => rows,
                Err(error) => {
                    let _ = tx.send(Err(map_error(error))).await;
                    return;
                }
            };
            if tx.send(Ok(QueryEvent::Columns(columns))).await.is_err() {
                return;
            }
            let mut rows = pin!(rows);
            let mut batch = Vec::new();
            let mut emitted = 0_u64;
            loop {
                if row_limit > 0 && emitted >= row_limit {
                    break;
                }
                match rows.next().await {
                    Some(Ok(row)) => {
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
                    }
                    Some(Err(error)) => {
                        let _ = tx.send(Err(map_error(error))).await;
                        return;
                    }
                    None => break,
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
        self.client
            .cancel_token()
            .cancel_query(tokio_postgres::NoTls)
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
impl TransactionControl for PostgresSession {
    async fn begin(&self, mode: TransactionMode) -> Result<(), DriverError> {
        let sql = match mode {
            TransactionMode::ReadWrite => "BEGIN",
            TransactionMode::ReadOnly => "BEGIN READ ONLY",
        };
        self.client.batch_execute(sql).await.map_err(map_error)?;
        self.set_state(TransactionState::Active);
        Ok(())
    }

    async fn commit(&self) -> Result<(), DriverError> {
        match self.client.batch_execute("COMMIT").await {
            Ok(()) => {
                self.set_state(TransactionState::Idle);
                Ok(())
            }
            Err(error) => {
                self.set_state(TransactionState::Failed);
                Err(map_error(error))
            }
        }
    }

    async fn rollback(&self) -> Result<(), DriverError> {
        match self.client.batch_execute("ROLLBACK").await {
            Ok(()) => {
                self.set_state(TransactionState::Idle);
                Ok(())
            }
            Err(error) => {
                self.set_state(TransactionState::Unknown);
                Err(map_error(error))
            }
        }
    }

    async fn savepoint(&self, name: &str) -> Result<(), DriverError> {
        validate_savepoint(name)?;
        self.client
            .batch_execute(&format!("SAVEPOINT {name}"))
            .await
            .map_err(map_error)
    }

    async fn rollback_to(&self, name: &str) -> Result<(), DriverError> {
        validate_savepoint(name)?;
        self.client
            .batch_execute(&format!("ROLLBACK TO SAVEPOINT {name}"))
            .await
            .map_err(map_error)
    }

    async fn release_savepoint(&self, name: &str) -> Result<(), DriverError> {
        validate_savepoint(name)?;
        self.client
            .batch_execute(&format!("RELEASE SAVEPOINT {name}"))
            .await
            .map_err(map_error)
    }

    fn state(&self) -> TransactionState {
        *self.tx_state.lock().expect("postgres tx state poisoned")
    }
}
