use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dexo_driver_api::{CapabilityState, DriverError, QueryId, QueryRequest, QueryStream, Session};
use dexo_mcp::router::{McpConnectionRouter, McpSessionSlot};

struct CountingSession {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl Session for CountingSession {
    fn capabilities(&self) -> &[CapabilityState] {
        &[]
    }

    async fn execute(&self, _: QueryRequest) -> Result<QueryStream, DriverError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(DriverError::unsupported("count only"))
    }

    async fn cancel(&self, _: QueryId) -> Result<(), DriverError> {
        Ok(())
    }

    async fn close(self: Box<Self>) -> Result<(), DriverError> {
        Ok(())
    }
}

impl CountingSession {
    fn query_calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[tokio::test]
async fn same_profile_routes_each_request_to_its_named_connection() {
    let sales = Arc::new(CountingSession {
        calls: AtomicUsize::new(0),
    });
    let audit = Arc::new(CountingSession {
        calls: AtomicUsize::new(0),
    });
    let mut allowed = BTreeMap::new();
    allowed.insert(
        "sales".into(),
        McpSessionSlot {
            session: Arc::clone(&sales) as Arc<dyn Session>,
        },
    );
    allowed.insert(
        "audit".into(),
        McpSessionSlot {
            session: Arc::clone(&audit) as Arc<dyn Session>,
        },
    );
    let router = McpConnectionRouter::new(allowed);
    let session = router.session("audit").await.unwrap();
    let _ = session.execute(QueryRequest::read("select 1", 1)).await;
    assert_eq!(sales.query_calls(), 0);
    assert_eq!(audit.query_calls(), 1);
    assert!(router.resolve(None).is_err());
    assert_eq!(router.resolve(Some("audit")).unwrap(), "audit");
}
