//! MCP (Model Context Protocol) client.
//!
//! v1 surface (ARY-2221, ADR-020):
//!
//! - [`protocol`] — JSON-RPC envelope types + NDJSON framing.
//! - [`error`] — [`McpError`] + [`McpResult`].
//! - [`dirs`] — `$JEKKO_HOME` resolution and default config path.
//! - [`config`] — `mcp.toml` parser/writer (atomic, comment-preserving).
//! - [`client`] — [`StdioClient`] for sequential request/response over a
//!   spawned child process.
//!
//! Used by `jekko mcp {list,attach,detach,status}` in `crates/jekko-cli/`.
//! Designed so other crates (the daemon, the agent loop) can drive an
//! attached MCP server once upstream wiring lands.

pub mod client;
pub mod config;
pub mod dirs;
pub mod error;
pub mod protocol;

// Re-exports for the historically-stable surface (preserves what the old
// single-file `mcp.rs` exposed, so any downstream consumers keep building).
pub use error::{McpError, McpResult};
pub use protocol::{
    decode_response, encode_request, request, McpErrorEnvelope, McpRequest, McpResponse,
    JSONRPC_VERSION,
};

pub use client::{McpTool, StdioClient};
pub use config::{
    load_or_empty, remove_server_entry, validate_server_name, write_server_entry, McpConfig,
    McpServerConfig, SERVER_NAME_PATTERN,
};
pub use dirs::{default_mcp_config_path, jekko_home, MCP_CONFIG_FILENAME};
