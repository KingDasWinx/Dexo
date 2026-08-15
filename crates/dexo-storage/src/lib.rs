mod catalog_cache;
mod connection;
mod database;
mod document;
mod history;
mod layout;
mod mcp;
mod mcp_profile;
mod migrations;
mod project;
mod recent;
mod recovery;
mod schema_snapshot;
mod session_recovery;
mod snippet;

pub use catalog_cache::CatalogCache;
pub use connection::{ConnectionRepository, ImportReport, export_portable, import_portable};
pub use database::{AppPaths, Database};
pub use document::{DocumentRepository, FileFingerprint, StoredDocument, has_external_conflict};
pub use history::HistoryRepository;
pub use layout::{LAYOUT_VERSION, LayoutRepository, Preferences, WorkbenchLayout};
pub use mcp::SqliteGrantLedger;
pub use mcp_profile::McpProfileRepository;
pub use migrations::{
    LATEST_SCHEMA_VERSION, MIGRATION_1, MIGRATION_2, MIGRATION_3, MIGRATION_4, MIGRATION_5,
    MIGRATION_6, MIGRATION_7, MIGRATION_8, MIGRATION_9, apply_pending,
};
pub use project::{ProjectDeletePreview, ProjectRepository};
pub use recent::RecentItemsRepository;
pub use recovery::{RecoveryDocument, RecoveryRepository};
pub use schema_snapshot::SchemaSnapshotStore;
pub use session_recovery::{SessionRecoveryRepository, SessionRecoveryState};
pub use snippet::SnippetRepository;
