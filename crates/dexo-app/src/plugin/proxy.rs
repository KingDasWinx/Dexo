//! A driver plugin as Dexo's other drivers are: a `ConnectionFactory` whose sessions
//! forward every call to the plugin's process.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dexo_driver_api::{
    BulkWriter, CapabilityState, CatalogList, CatalogListOptions, CatalogObject, CatalogReader,
    ColumnKeyInfo, ConnectRequest, ConnectionFactory, DataMutator, DataPage, DataRequest, DbValue,
    DriverDescriptor, DriverError, DriverErrorCategory, ExplainPlan, ExplainProvider,
    ExplainRequest, ForeignKeyRef, Mutation, ObjectDdl, ObjectId, QualifiedName, QueryEvent,
    QueryId, QueryRequest, QueryStream, RemoteValueRef, Session, TransactionControl,
    TransactionMode, TransactionState,
};
use dexo_plugin_protocol::{
    Bytes, Cursor, Family, OnSession, QueryExecute, QueryPage, SessionOpenParams,
    SessionOpenResult, TxResult, method,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::host::{CANCEL, Live, PluginHost};
use super::lock;

pub struct PluginFactory {
    descriptor: DriverDescriptor,
    host: Arc<PluginHost>,
}

impl PluginFactory {
    pub fn new(descriptor: DriverDescriptor, host: Arc<PluginHost>) -> Self {
        Self { descriptor, host }
    }

    pub fn host(&self) -> &Arc<PluginHost> {
        &self.host
    }
}

#[async_trait::async_trait]
impl ConnectionFactory for PluginFactory {
    fn descriptor(&self) -> DriverDescriptor {
        self.descriptor.clone()
    }

    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        // Counted from the start: an idle timer running out meanwhile leaves it be.
        self.host.session_opened();
        let opening = async {
            let live = self.host.live().await?;
            let opened: SessionOpenResult = self
                .host
                .ask(
                    &live,
                    method::SESSION_OPEN,
                    SessionOpenParams::new(&self.descriptor.id, &request),
                    None,
                )
                .await?;
            Ok::<_, DriverError>((live, opened))
        };
        let (live, opened) = match opening.await {
            Ok(opened) => opened,
            Err(error) => {
                self.host.session_closed();
                return Err(error);
            }
        };
        Ok(Box::new(ProxySession {
            host: self.host.clone(),
            live,
            id: opened.session,
            capabilities: opened.capabilities,
            families: opened.families,
            server_session_id: opened.server_session_id,
            transaction: Arc::new(Mutex::new(Tx {
                state: opened.transaction.unwrap_or(TransactionState::Unknown),
                notice: None,
            })),
            closed: AtomicBool::new(false),
        }))
    }
}

/// The transaction state and notice the plugin last reported.
struct Tx {
    state: TransactionState,
    notice: Option<String>,
}

pub struct ProxySession {
    host: Arc<PluginHost>,
    live: Arc<Live>,
    id: String,
    capabilities: Vec<CapabilityState>,
    families: Vec<Family>,
    server_session_id: Option<String>,
    transaction: Arc<Mutex<Tx>>,
    closed: AtomicBool,
}

impl ProxySession {
    async fn call<R: DeserializeOwned>(
        &self,
        method: &str,
        args: impl Serialize + Send,
    ) -> Result<R, DriverError> {
        let params = OnSession {
            session: self.id.clone(),
            args,
        };
        self.host.ask(&self.live, method, params, None).await
    }

    fn has(&self, family: Family) -> bool {
        self.families.contains(&family)
    }

    async fn tx(&self, method: &str, args: impl Serialize + Send) -> Result<(), DriverError> {
        let result: TxResult = self.call(method, args).await?;
        {
            let mut tx = lock(&self.transaction);
            tx.state = result.state;
            tx.notice = result.notice;
        }
        result.error.map_or(Ok(()), Err)
    }
}

#[async_trait::async_trait]
impl Session for ProxySession {
    fn capabilities(&self) -> &[CapabilityState] {
        &self.capabilities
    }

    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError> {
        let first: QueryPage = self
            .call(method::QUERY_EXECUTE, QueryExecute::new(&request))
            .await?;
        let mut pages = Pages {
            host: self.host.clone(),
            live: self.live.clone(),
            session: self.id.clone(),
            query: request.id,
            events: VecDeque::new(),
            cursor: None,
            error: None,
            transaction: self.transaction.clone(),
        };
        pages.take(first);
        Ok(Box::pin(pages.into_stream()))
    }

    /// A driver that does not answer a cancel in time is killed: a query nobody can stop
    /// would otherwise hold the session for good.
    async fn cancel(&self, query: QueryId) -> Result<(), DriverError> {
        let params = OnSession {
            session: self.id.clone(),
            args: query.0,
        };
        match self
            .host
            .ask::<()>(&self.live, method::QUERY_CANCEL, params, Some(CANCEL))
            .await
        {
            Err(error) if error.category() == DriverErrorCategory::Timeout => {
                let why = "it did not answer a cancel within 5 seconds";
                self.host.kill(&self.live, why);
                Err(DriverError::new(
                    DriverErrorCategory::Network,
                    format!("the {} plugin stopped: {why}", self.host.name()),
                ))
            }
            other => other,
        }
    }

    async fn close(self: Box<Self>) -> Result<(), DriverError> {
        self.closed.store(true, Ordering::SeqCst);
        let result = self.call::<()>(method::SESSION_CLOSE, ()).await;
        self.host.session_closed();
        result
    }

    fn transactions(&self) -> Option<&dyn TransactionControl> {
        self.has(Family::Transactions)
            .then_some(self as &dyn TransactionControl)
    }

    fn catalog(&self) -> Option<&dyn CatalogReader> {
        self.has(Family::Catalog)
            .then_some(self as &dyn CatalogReader)
    }

    fn data(&self) -> Option<&dyn DataMutator> {
        self.has(Family::Data).then_some(self as &dyn DataMutator)
    }

    fn bulk(&self) -> Option<&dyn BulkWriter> {
        self.has(Family::Bulk).then_some(self as &dyn BulkWriter)
    }

    fn explain(&self) -> Option<&dyn ExplainProvider> {
        self.has(Family::Explain)
            .then_some(self as &dyn ExplainProvider)
    }

    fn server_session_id(&self) -> Option<String> {
        self.server_session_id.clone()
    }
}

impl Drop for ProxySession {
    /// A session dropped without `close` still frees its place in the plugin.
    fn drop(&mut self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.live.rpc.notify(
                method::SESSION_CLOSE,
                OnSession {
                    session: self.id.clone(),
                    args: (),
                },
            );
            self.host.session_closed();
        }
    }
}

/// A result read page by page as it is polled: the next page is asked for only once the
/// last is used up. Dropped before its end, it cancels the query and frees the cursor.
struct Pages {
    host: Arc<PluginHost>,
    live: Arc<Live>,
    session: String,
    query: QueryId,
    events: VecDeque<QueryEvent>,
    cursor: Option<String>,
    error: Option<DriverError>,
    transaction: Arc<Mutex<Tx>>,
}

impl Pages {
    fn take(&mut self, page: QueryPage) {
        self.events.extend(page.events);
        self.cursor = if page.done { None } else { page.cursor };
        self.error = page.error;
        if let Some(state) = page.transaction {
            lock(&self.transaction).state = state;
        }
    }

    fn into_stream(
        self,
    ) -> impl futures_util::Stream<Item = Result<QueryEvent, DriverError>> + Send {
        futures_util::stream::unfold(self, |mut pages| async move {
            loop {
                if let Some(event) = pages.events.pop_front() {
                    return Some((Ok(event), pages));
                }
                if let Some(error) = pages.error.take() {
                    return Some((Err(error), pages));
                }
                let cursor = pages.cursor.clone()?;
                let next = pages
                    .host
                    .ask::<QueryPage>(&pages.live, method::QUERY_FETCH, Cursor { cursor }, None)
                    .await;
                match next {
                    Ok(page) => pages.take(page),
                    Err(error) => {
                        pages.cursor = None;
                        return Some((Err(error), pages));
                    }
                }
            }
        })
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        if let Some(cursor) = self.cursor.take() {
            self.live.rpc.notify(
                method::QUERY_CANCEL,
                OnSession {
                    session: self.session.clone(),
                    args: self.query.0,
                },
            );
            self.live.rpc.notify(method::QUERY_CLOSE, Cursor { cursor });
        }
    }
}

#[async_trait::async_trait]
impl CatalogReader for ProxySession {
    async fn list_children(
        &self,
        parent: Option<&ObjectId>,
        options: &CatalogListOptions,
    ) -> Result<CatalogList, DriverError> {
        self.call(method::CATALOG_LIST_CHILDREN, (parent, options))
            .await
    }

    async fn object(&self, id: &ObjectId) -> Result<Option<CatalogObject>, DriverError> {
        self.call(method::CATALOG_OBJECT, id).await
    }

    async fn ddl(&self, id: &ObjectId) -> Result<ObjectDdl, DriverError> {
        self.call(method::CATALOG_DDL, id).await
    }

    async fn dependencies(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        self.call(method::CATALOG_DEPENDENCIES, id).await
    }

    async fn dependents(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        self.call(method::CATALOG_DEPENDENTS, id).await
    }

    async fn foreign_keys(&self, table: &QualifiedName) -> Result<Vec<ForeignKeyRef>, DriverError> {
        self.call(method::CATALOG_FOREIGN_KEYS, table).await
    }

    async fn relations_named(
        &self,
        schema: Option<&str>,
        name: &str,
    ) -> Result<Option<Vec<CatalogObject>>, DriverError> {
        self.call(method::CATALOG_RELATIONS_NAMED, (schema, name))
            .await
    }

    async fn databases(&self) -> Result<Vec<String>, DriverError> {
        self.call(method::CATALOG_DATABASES, ()).await
    }
}

#[async_trait::async_trait]
impl DataMutator for ProxySession {
    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError> {
        self.call(method::DATA_FETCH, request).await
    }

    async fn fetch_value(
        &self,
        value: &RemoteValueRef,
        offset: u64,
        limit: u32,
    ) -> Result<Vec<u8>, DriverError> {
        let bytes: Bytes = self
            .call(method::DATA_FETCH_VALUE, (value, offset, limit))
            .await?;
        Ok(bytes.0)
    }

    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError> {
        self.call(method::DATA_APPLY, mutations).await
    }

    async fn table_columns(
        &self,
        target: &QualifiedName,
    ) -> Result<Vec<ColumnKeyInfo>, DriverError> {
        self.call(method::DATA_TABLE_COLUMNS, target).await
    }

    async fn estimate_rows(&self, target: &QualifiedName) -> Result<Option<u64>, DriverError> {
        self.call(method::DATA_ESTIMATE_ROWS, target).await
    }
}

#[async_trait::async_trait]
impl BulkWriter for ProxySession {
    async fn insert_batch(
        &self,
        table: &QualifiedName,
        columns: &[String],
        rows: &[Vec<DbValue>],
    ) -> Result<u64, DriverError> {
        self.call(method::BULK_INSERT_BATCH, (table, columns, rows))
            .await
    }
}

#[async_trait::async_trait]
impl ExplainProvider for ProxySession {
    async fn explain(&self, request: ExplainRequest) -> Result<ExplainPlan, DriverError> {
        self.call(method::EXPLAIN, request).await
    }
}

#[async_trait::async_trait]
impl TransactionControl for ProxySession {
    async fn begin(&self, mode: TransactionMode) -> Result<(), DriverError> {
        self.tx(method::TX_BEGIN, mode).await
    }

    async fn commit(&self) -> Result<(), DriverError> {
        self.tx(method::TX_COMMIT, ()).await
    }

    async fn rollback(&self) -> Result<(), DriverError> {
        self.tx(method::TX_ROLLBACK, ()).await
    }

    async fn savepoint(&self, name: &str) -> Result<(), DriverError> {
        self.tx(method::TX_SAVEPOINT, name).await
    }

    async fn rollback_to(&self, name: &str) -> Result<(), DriverError> {
        self.tx(method::TX_ROLLBACK_TO, name).await
    }

    async fn release_savepoint(&self, name: &str) -> Result<(), DriverError> {
        self.tx(method::TX_RELEASE_SAVEPOINT, name).await
    }

    fn state(&self) -> TransactionState {
        lock(&self.transaction).state
    }

    fn take_notice(&self) -> Option<String> {
        lock(&self.transaction).notice.take()
    }
}
