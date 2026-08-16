use std::sync::Arc;

use dexo_driver_api::{ConnectRequest, ConnectionFactory, QueryRequest, QueryStream, Session};
use tokio::sync::Mutex;

use crate::error::{AppError, ErrorCategory};
use crate::query_service::map_driver_error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    Connecting,
    Ready,
    Transaction,
    FailedTransaction,
    Unknown,
    Closed,
}

pub struct SessionManager {
    factory: Arc<dyn ConnectionFactory>,
    request: ConnectRequest,
    session: Mutex<Option<Box<dyn Session>>>,
    state: Mutex<SessionState>,
}

impl SessionManager {
    pub fn new(factory: Arc<dyn ConnectionFactory>, request: ConnectRequest) -> Self {
        Self {
            factory,
            request,
            session: Mutex::new(None),
            state: Mutex::new(SessionState::Closed),
        }
    }

    pub async fn state(&self) -> SessionState {
        *self.state.lock().await
    }

    pub async fn execute_mutating(&self, sql: &str) -> Result<QueryStream, AppError> {
        self.ensure_ready().await?;
        *self.state.lock().await = SessionState::Transaction;
        let result = {
            let guard = self.session.lock().await;
            let session = guard.as_ref().expect("connected session");
            session.execute(QueryRequest::write(sql)).await
        };
        match result {
            Ok(stream) => Ok(stream),
            Err(error) => {
                *self.state.lock().await = SessionState::Unknown;
                Err(map_driver_error(error))
            }
        }
    }

    pub async fn execute_read(&self, sql: &str, row_limit: u64) -> Result<QueryStream, AppError> {
        if let Err(error) = self.ensure_ready().await {
            if matches!(error.category(), ErrorCategory::Network) {
                self.connect().await?;
            } else {
                return Err(error);
            }
        }
        let result = {
            let guard = self.session.lock().await;
            let session = guard.as_ref().expect("connected session");
            session.execute(QueryRequest::read(sql, row_limit)).await
        };
        result.map_err(map_driver_error)
    }

    async fn ensure_ready(&self) -> Result<(), AppError> {
        if self.session.lock().await.is_some() {
            return Ok(());
        }
        self.connect().await?;
        Ok(())
    }

    async fn connect(&self) -> Result<(), AppError> {
        *self.state.lock().await = SessionState::Connecting;
        match self.factory.connect(self.request.clone()).await {
            Ok(session) => {
                *self.session.lock().await = Some(session);
                *self.state.lock().await = SessionState::Ready;
                Ok(())
            }
            Err(error) => {
                *self.state.lock().await = SessionState::Closed;
                Err(map_driver_error(error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use dexo_driver_api::{
        ConnectRequest, ConnectionFactory, DriverError, DriverErrorCategory, QueryId, QueryRequest,
        QueryStream, Session,
    };
    use secrecy::SecretString;

    use super::{SessionManager, SessionState};
    use crate::error::AppError;

    struct DisconnectAfterExecute {
        executions: Arc<AtomicUsize>,
    }

    struct DisconnectSession {
        executions: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl ConnectionFactory for DisconnectAfterExecute {
        fn descriptor(&self) -> dexo_driver_api::DriverDescriptor {
            dexo_driver_api::DriverDescriptor {
                id: "fake",
                display_name: "Fake",
                default_port: 1,
                options: dexo_driver_api::ConnectionOptions {
                    tls: false,
                    client_certificate: false,
                    ssh: false,
                    proxy: false,
                },
            }
        }

        async fn connect(&self, _: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
            Ok(Box::new(DisconnectSession {
                executions: Arc::clone(&self.executions),
            }))
        }
    }

    #[async_trait::async_trait]
    impl Session for DisconnectSession {
        fn capabilities(&self) -> &[dexo_driver_api::CapabilityState] {
            &[]
        }

        async fn execute(&self, _: QueryRequest) -> Result<QueryStream, DriverError> {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Err(DriverError::new(
                DriverErrorCategory::Network,
                "disconnected",
            ))
        }

        async fn cancel(&self, _: QueryId) -> Result<(), DriverError> {
            Ok(())
        }

        async fn close(self: Box<Self>) -> Result<(), DriverError> {
            Ok(())
        }
    }

    fn manager_with(driver: DisconnectAfterExecute) -> (SessionManager, Arc<AtomicUsize>) {
        let executions = Arc::clone(&driver.executions);
        (
            SessionManager::new(
                Arc::new(driver),
                ConnectRequest::new(
                    "127.0.0.1:1",
                    None,
                    "u".into(),
                    SecretString::from("x"),
                    false,
                ),
            ),
            executions,
        )
    }

    #[tokio::test]
    async fn disconnect_during_transaction_never_retries_statement() {
        let executions = Arc::new(AtomicUsize::new(0));
        let (manager, executions) = manager_with(DisconnectAfterExecute {
            executions: Arc::clone(&executions),
        });
        let result = manager
            .execute_mutating("update accounts set balance=0")
            .await;
        assert!(matches!(result, Err(AppError { .. })));
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(manager.state().await, SessionState::Unknown);
    }
}
