//! Error types for the MCP client.
//!
//! Strongly-typed so callers can match on the failure mode (e.g. surface a
//! timeout differently from a protocol violation, or refuse to create a
//! duplicate-name attach without confusing it with a transport error).

use std::path::PathBuf;

use thiserror::Error;

/// Result alias for MCP operations.
pub type McpResult<T> = std::result::Result<T, McpError>;

/// All failure modes the MCP client can surface.
#[derive(Debug, Error)]
pub enum McpError {
    /// The config file could not be read or parsed.
    #[error("mcp config parse error at {path}: {message}")]
    ConfigParse {
        /// Path the parser was reading.
        path: PathBuf,
        /// Parser-supplied message.
        message: String,
    },

    /// The config file did not exist where expected.
    #[error("mcp config not found: {0}")]
    ConfigNotFound(PathBuf),

    /// Writing the config back to disk failed.
    #[error("mcp config write error at {path}: {source}")]
    ConfigWrite {
        /// Target path.
        path: PathBuf,
        /// Underlying io error.
        #[source]
        source: std::io::Error,
    },

    /// An attach attempt collided with an existing server name.
    #[error("mcp server `{0}` already exists; detach first or pick a different name")]
    DuplicateName(String),

    /// The named server is not in the config.
    #[error("mcp server `{0}` not found in config")]
    UnknownServer(String),

    /// Failed to spawn the configured child process.
    #[error("mcp spawn failed for `{name}`: {source}")]
    Spawn {
        /// Server name being launched.
        name: String,
        /// Underlying io error.
        #[source]
        source: std::io::Error,
    },

    /// Stdio read/write or framing failure on an attached server.
    #[error("mcp transport error: {0}")]
    Transport(String),

    /// Wall-clock timeout waiting on a server response.
    #[error("mcp request timed out after {0}s")]
    Timeout(u64),

    /// Server response did not match the JSON-RPC envelope contract.
    #[error("mcp protocol violation: {0}")]
    ProtocolViolation(String),

    /// Server returned a structured JSON-RPC error.
    #[error("mcp server error (code {code}): {message}")]
    ServerError {
        /// JSON-RPC error code.
        code: i64,
        /// Human-readable message.
        message: String,
    },

    /// Child process exited before answering. Stderr tail is captured for
    /// diagnostic surfacing without leaking the whole stream.
    #[error("mcp server `{name}` exited before responding ({stderr_tail:?})")]
    EarlyExit {
        /// Server name.
        name: String,
        /// Last ~1KB of the child's stderr (UTF-8 lossy).
        stderr_tail: String,
    },

    /// The provided name is not acceptable per the charset policy.
    #[error("mcp server name `{0}` rejected: must match {1}")]
    InvalidName(String, &'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_contains_path() {
        let e = McpError::ConfigNotFound(PathBuf::from("/tmp/x.toml"));
        assert!(e.to_string().contains("/tmp/x.toml"));
    }

    #[test]
    fn duplicate_name_surfaces_name() {
        let e = McpError::DuplicateName("aara".to_string());
        assert!(e.to_string().contains("aara"));
    }

    #[test]
    fn server_error_surfaces_code() {
        let e = McpError::ServerError {
            code: -32601,
            message: "Method not found".into(),
        };
        let s = e.to_string();
        assert!(s.contains("-32601"));
        assert!(s.contains("Method not found"));
    }
}
