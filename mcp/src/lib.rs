pub mod mcp;

pub use mcp::{McpClient, McpError};

// Re-export commonly used types from rmcp for convenience
pub use rmcp::model::{Prompt, Resource, Tool};
