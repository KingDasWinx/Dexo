pub mod admin_service;
pub mod catalog_service;
pub mod connection_policy;
pub mod connection_profile;
pub mod connection_service;
pub mod data;
pub mod diagnostic_service;
pub mod driver_registry;
pub mod error;
pub mod event;
pub mod explain_service;
pub mod mcp;
pub mod project;
pub mod query_service;
pub mod recovery_service;
pub mod schema;
pub mod schema_diff;
pub mod script;
pub mod search_service;
pub mod session_manager;
pub mod transaction_service;
pub mod transfer;

pub use catalog_service::{CatalogService, SnapshotCatalog, parse_qualified};
pub use connection_policy::{ConnectionPolicy, ConnectionPolicyOverrides, Environment};
pub use connection_profile::{
    ConnectionId, ConnectionProfile, PURPOSE_DATABASE_PASSWORD, SecretRef,
};
pub use connection_service::{
    ConnectionProfiles, NewConnection, SecretPersist, create as create_connection,
    set_secret as set_connection_secret, test_input as test_connection_input,
};
pub use driver_registry::DriverRegistry;
pub use error::{AppError, ErrorCategory};
pub use project::{Project, ProjectId};
pub use query_service::{QueryService, QueryTask, map_driver_error};
pub use script::{ExecutionTarget, ScriptPolicy, statements_for};
pub use session_manager::{SessionManager, SessionState};
pub use transaction_service::TransactionService;
