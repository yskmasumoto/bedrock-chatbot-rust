pub mod config;
pub mod mcp;

pub use config::{McpConfig, ServerConfig};
pub use mcp::{McpClient, McpError};

// Re-export commonly used types from rmcp for convenience
pub use rmcp::model::{Prompt, Resource, Tool};
