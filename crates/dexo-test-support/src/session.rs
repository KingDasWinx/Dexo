use std::sync::{Arc, Mutex};

use dexo_driver_api::{
    CapabilityState, ColumnKeyInfo, ColumnMeta, DataMutator, DataPage, DataRequest, DbValue,
    DdlExecutor, DdlOutcome, DdlPlan, DriverError, Mutation, QualifiedName, QueryEvent, QueryId,
    QueryRequest, QueryStream, RemoteValueRef, RowBatch, SchemaChange, Session, TransactionControl,
    TransactionMode, TransactionState,
};

/// A session that answers from memory and writes down every call, so a test can assert
/// what reached the "server" and in which order.
#[derive(Clone, Default)]
pub struct FakeSession {
    log: Arc<Mutex<Vec<String>>>,
    columns: Vec<String>,
    rows: Vec<Vec<DbValue>>,
    keys: Vec<ColumnKeyInfo>,
    hang: bool,
}

impl FakeSession {
    pub fn with_rows(columns: &[&str], rows: Vec<Vec<DbValue>>) -> Self {
        Self {
            columns: columns.iter().map(|name| name.to_string()).collect(),
            rows,
            ..Self::default()
        }
    }

    /// `execute` returns a stream that never yields, for cancellation tests.
    pub fn hanging() -> Self {
        Self {
            hang: true,
            ..Self::default()
        }
    }

    pub fn with_keys(mut self, keys: Vec<ColumnKeyInfo>) -> Self {
        self.keys = keys;
        self
    }

    pub fn log(&self) -> Vec<String> {
        self.log.lock().expect("fake session log").clone()
    }

    fn record(&self, entry: impl Into<String>) {
        self.log
            .lock()
            .expect("fake session log")
            .push(entry.into());
    }

    fn column_meta(&self) -> Vec<ColumnMeta> {
        self.columns
            .iter()
            .map(|name| ColumnMeta {
                name: name.clone(),
                type_name: "text".into(),
                nullable: true,
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl Session for FakeSession {
    fn capabilities(&self) -> &[CapabilityState] {
        &[]
    }

    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError> {
        self.record(format!("execute {}", request.sql));
        if self.hang {
            return Ok(Box::pin(futures_util::stream::pending::<
                Result<QueryEvent, DriverError>,
            >()));
        }
        let limit = if request.row_limit == 0 {
            usize::MAX
        } else {
            request.row_limit as usize
        };
        let events = vec![
            Ok(QueryEvent::Columns(self.column_meta())),
            Ok(QueryEvent::Rows(RowBatch {
                rows: self.rows.iter().take(limit).cloned().collect(),
            })),
            Ok(QueryEvent::Finished {
                rows_affected: Some(self.rows.len() as u64),
            }),
        ];
        Ok(Box::pin(futures_util::stream::iter(events)))
    }

    async fn cancel(&self, _query: QueryId) -> Result<(), DriverError> {
        self.record("cancel");
        Ok(())
    }

    async fn close(self: Box<Self>) -> Result<(), DriverError> {
        Ok(())
    }

    fn transactions(&self) -> Option<&dyn TransactionControl> {
        Some(self)
    }

    fn data(&self) -> Option<&dyn DataMutator> {
        Some(self)
    }

    fn ddl(&self) -> Option<&dyn DdlExecutor> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl TransactionControl for FakeSession {
    async fn begin(&self, mode: TransactionMode) -> Result<(), DriverError> {
        self.record(format!("begin {mode:?}"));
        Ok(())
    }

    async fn commit(&self) -> Result<(), DriverError> {
        self.record("commit");
        Ok(())
    }

    async fn rollback(&self) -> Result<(), DriverError> {
        self.record("rollback");
        Ok(())
    }

    async fn savepoint(&self, name: &str) -> Result<(), DriverError> {
        self.record(format!("savepoint {name}"));
        Ok(())
    }

    async fn rollback_to(&self, name: &str) -> Result<(), DriverError> {
        self.record(format!("rollback_to {name}"));
        Ok(())
    }

    async fn release_savepoint(&self, name: &str) -> Result<(), DriverError> {
        self.record(format!("release {name}"));
        Ok(())
    }

    fn state(&self) -> TransactionState {
        TransactionState::Idle
    }
}

#[async_trait::async_trait]
impl DataMutator for FakeSession {
    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError> {
        self.record(format!(
            "fetch {} {}+{}",
            request.object.display_unquoted(),
            request.page.offset,
            request.page.limit
        ));
        let rows = self
            .rows
            .iter()
            .skip(request.page.offset as usize)
            .take(request.page.limit as usize + 1)
            .cloned()
            .collect();
        Ok(DataPage::from_fetched(
            self.column_meta(),
            rows,
            request.page.offset,
            request.page.limit,
        ))
    }

    async fn fetch_value(
        &self,
        _value: &RemoteValueRef,
        _offset: u64,
        _limit: u32,
    ) -> Result<Vec<u8>, DriverError> {
        Ok(Vec::new())
    }

    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError> {
        self.record(format!("apply {}", mutations.len()));
        Ok(())
    }

    async fn table_columns(
        &self,
        target: &QualifiedName,
    ) -> Result<Vec<ColumnKeyInfo>, DriverError> {
        self.record(format!("columns {}", target.display_unquoted()));
        Ok(self.keys.clone())
    }
}

#[async_trait::async_trait]
impl DdlExecutor for FakeSession {
    fn plan_change(&self, _change: &SchemaChange) -> Result<DdlPlan, DriverError> {
        Ok(DdlPlan::default())
    }

    async fn apply_ddl(&self, plan: &DdlPlan) -> Result<DdlOutcome, DriverError> {
        for statement in &plan.statements {
            self.record(format!("ddl {}", statement.sql));
        }
        Ok(DdlOutcome::Committed)
    }
}
