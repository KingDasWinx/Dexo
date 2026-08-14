pub mod error;
pub mod prompts;
pub mod resources;
pub mod schema;
pub mod server;
pub mod stdio;
pub mod tools_read;

pub use error::hidden_error;
pub use server::DexoMcpServer;
pub use stdio::{init_mcp_tracing, serve_io, serve_profile, serve_with_session};
