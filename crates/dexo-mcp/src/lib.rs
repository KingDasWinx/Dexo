pub mod backend;
pub mod error;
pub mod prompts;
pub mod render;
pub mod resources;
pub mod router;
pub mod schema;
pub mod server;
pub mod stdio;
pub mod tools_read;
pub mod tools_write;

pub use backend::McpBackend;
pub use error::hidden_error;
pub use server::{DexoMcpServer, TOOL_SCHEMA_VERSION};
pub use stdio::{init_mcp_tracing, serve_io, serve_stdio};
