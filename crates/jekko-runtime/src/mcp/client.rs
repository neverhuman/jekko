//! Stdio MCP client.
//!
//! Spawns the configured child process, sends JSON-RPC requests as
//! newline-delimited JSON on stdin, reads responses one line at a time
//! from stdout, and surfaces stderr (last ~1 KB) on early exit.
//!
//! v1 supports sequential request/response only — one call in flight at a
//! time, which is sufficient for `initialize`, `tools/list`, and `status`
//! probes. Concurrent calls would require id correlation; deferred.
//!
//! DoS guards:
//! - `MAX_LINE_BYTES = 4 MiB` ceiling on any single response line; an
//!   over-long line errors with [`McpError::ProtocolViolation`] rather
//!   than allocating unbounded memory.
//! - `STDERR_TAIL_BYTES = 1 KiB` ring of stderr; never holds more.
//! - Per-call timeout via `tokio::time::timeout` — caller passes seconds.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use super::config::McpServerConfig;
use super::error::{McpError, McpResult};
use super::protocol::{decode_response, encode_request, request, McpResponse};

/// Hard ceiling on a single response line in bytes. 4 MiB.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Stderr tail length in bytes (kept for surfacing on early exit).
pub const STDERR_TAIL_BYTES: usize = 1024;

/// One MCP tool descriptor as returned by `tools/list`.
#[derive(Debug, Clone, Deserialize)]
pub struct McpTool {
    /// Tool name (e.g. `"aara_orchestrate"`).
    pub name: String,
    /// Optional human-readable description.
    #[serde(default)]
    pub description: Option<String>,
}

/// A spawned MCP server, ready for sequential request/response.
pub struct StdioClient {
    name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
    next_id: u64,
}

impl StdioClient {
    /// Spawn the configured child and prepare for handshake. The caller is
    /// responsible for invoking [`Self::initialize`] before sending other
    /// requests.
    pub async fn spawn(name: &str, cfg: &McpServerConfig) -> McpResult<Self> {
        if cfg.transport != "stdio" {
            return Err(McpError::Transport(format!(
                "unsupported transport `{}`; v1 supports stdio only",
                cfg.transport
            )));
        }
        let mut cmd = Command::new(&cfg.command);
        cmd.args(&cfg.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in cfg.resolved_env() {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().map_err(|e| McpError::Spawn {
            name: name.to_string(),
            source: e,
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("child stdin not piped".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("child stdout not piped".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| McpError::Transport("child stderr not piped".into()))?;

        let stderr_tail: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let tail_clone = stderr_tail.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut buf = [0u8; 256];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut tail = tail_clone.lock().await;
                        tail.extend_from_slice(&buf[..n]);
                        if tail.len() > STDERR_TAIL_BYTES {
                            let drop = tail.len() - STDERR_TAIL_BYTES;
                            tail.drain(..drop);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(StdioClient {
            name: name.to_string(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr_tail,
            next_id: 0,
        })
    }

    fn next_request_id(&mut self) -> String {
        self.next_id += 1;
        self.next_id.to_string()
    }

    /// Send the MCP `initialize` request and validate the response. Per
    /// spec 2024-11-05.
    pub async fn initialize(&mut self, timeout_secs: u64) -> McpResult<serde_json::Value> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "jekko",
                "version": env!("CARGO_PKG_VERSION"),
            },
        });
        let id = self.next_request_id();
        let req = request(id, "initialize", params);
        let resp = self.exchange(&req, timeout_secs).await?;
        let result = match resp.result {
            Some(v) => v,
            None => match resp.error {
                Some(e) => {
                    return Err(McpError::ServerError {
                        code: e.code,
                        message: e.message,
                    })
                }
                None => {
                    return Err(McpError::ProtocolViolation(
                        "initialize response has neither result nor error".into(),
                    ))
                }
            },
        };
        // Send the `initialized` notification per MCP spec (no response).
        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        });
        let mut bytes =
            serde_json::to_vec(&notif).map_err(|e| McpError::ProtocolViolation(e.to_string()))?;
        bytes.push(b'\n');
        self.stdin
            .write_all(&bytes)
            .await
            .map_err(|e| McpError::Transport(e.to_string()))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| McpError::Transport(e.to_string()))?;
        Ok(result)
    }

    /// Send `tools/list` and return the parsed tool array.
    pub async fn list_tools(&mut self, timeout_secs: u64) -> McpResult<Vec<McpTool>> {
        let id = self.next_request_id();
        let req = request(id, "tools/list", serde_json::json!({}));
        let resp = self.exchange(&req, timeout_secs).await?;
        let result = match resp.result {
            Some(v) => v,
            None => match resp.error {
                Some(e) => {
                    return Err(McpError::ServerError {
                        code: e.code,
                        message: e.message,
                    })
                }
                None => {
                    return Err(McpError::ProtocolViolation(
                        "tools/list response has neither result nor error".into(),
                    ))
                }
            },
        };
        let tools = result
            .get("tools")
            .ok_or_else(|| {
                McpError::ProtocolViolation("tools/list response missing `tools` field".into())
            })?
            .clone();
        let parsed: Vec<McpTool> = serde_json::from_value(tools)
            .map_err(|e| McpError::ProtocolViolation(format!("tools/list parse failure: {e}")))?;
        Ok(parsed)
    }

    /// Low-level: write a request line, read a response line, decode it.
    /// Surfaces early-exit with stderr_tail.
    async fn exchange(
        &mut self,
        req: &super::protocol::McpRequest,
        timeout_secs: u64,
    ) -> McpResult<McpResponse> {
        let bytes = encode_request(req)?;
        self.stdin
            .write_all(&bytes)
            .await
            .map_err(|e| McpError::Transport(e.to_string()))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| McpError::Transport(e.to_string()))?;

        let read_fut = read_bounded_line(&mut self.stdout, MAX_LINE_BYTES);
        let line = match tokio::time::timeout(Duration::from_secs(timeout_secs), read_fut).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => {
                // EOF: child closed stdout.
                let tail = self.stderr_tail.lock().await;
                let tail_str = String::from_utf8_lossy(&tail).to_string();
                return Err(McpError::EarlyExit {
                    name: self.name.clone(),
                    stderr_tail: tail_str,
                });
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(McpError::Timeout(timeout_secs)),
        };
        decode_response(&line)
    }

    /// Best-effort shutdown: close stdin, wait briefly for the child, then
    /// rely on `kill_on_drop` if it has not exited.
    pub async fn shutdown(mut self) -> McpResult<()> {
        // Close stdin to signal EOF.
        drop(self.stdin);
        let _ = tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await;
        Ok(())
    }
}

/// Read one '\n'-terminated line from the reader, capped at `max_bytes`.
/// Returns `Ok(None)` on EOF before any byte was read; returns
/// `Err(ProtocolViolation)` if the line exceeds the cap before terminating.
async fn read_bounded_line(
    reader: &mut BufReader<ChildStdout>,
    max_bytes: usize,
) -> McpResult<Option<String>> {
    let mut buf = Vec::with_capacity(256);
    loop {
        if buf.len() >= max_bytes {
            return Err(McpError::ProtocolViolation(format!(
                "response line exceeded {max_bytes} bytes without newline"
            )));
        }
        let take = max_bytes - buf.len();
        let mut chunk = Vec::new();
        let mut limited = (&mut *reader).take(take as u64);
        match limited.read_until(b'\n', &mut chunk).await {
            Ok(0) => {
                if buf.is_empty() {
                    return Ok(None);
                }
                // EOF before newline — incomplete frame.
                return Err(McpError::ProtocolViolation(
                    "stream closed mid-frame (no terminating newline)".into(),
                ));
            }
            Ok(_) => {
                let saw_nl = chunk.last() == Some(&b'\n');
                buf.extend_from_slice(&chunk);
                if saw_nl {
                    break;
                }
                // Hit the take cap without a newline; loop & re-take.
            }
            Err(e) => return Err(McpError::Transport(e.to_string())),
        }
    }
    let line = String::from_utf8(buf)
        .map_err(|e| McpError::ProtocolViolation(format!("non-utf8 response: {e}")))?;
    Ok(Some(line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn echo_python_cfg(script: &str) -> McpServerConfig {
        McpServerConfig {
            transport: "stdio".into(),
            command: "python3".into(),
            args: vec!["-c".into(), script.into()],
            env: BTreeMap::new(),
            timeouts: BTreeMap::new(),
        }
    }

    /// A minimal MCP-style stdio server: read one line, echo back a valid
    /// JSON-RPC response with the same id and a `{"tools": []}` result.
    const ECHO_SERVER: &str = r#"
import json, sys
line = sys.stdin.readline()
req = json.loads(line)
resp = {"jsonrpc": "2.0", "id": req["id"], "result": {"tools": []}}
print(json.dumps(resp), flush=True)
"#;

    #[tokio::test]
    async fn round_trip_initialize_against_echo_server() {
        let cfg = echo_python_cfg(ECHO_SERVER);
        let mut client = StdioClient::spawn("echo", &cfg).await.unwrap();
        let res = client.initialize(5).await.unwrap();
        // The echo doesn't return capabilities, just empty tools — but the
        // request id-correlation and framing should work.
        assert!(res.get("tools").is_some() || res.is_object());
    }

    #[tokio::test]
    async fn timeout_when_server_sleeps_forever() {
        let cfg = echo_python_cfg(
            r#"
import time, sys
time.sleep(60)
"#,
        );
        let mut client = StdioClient::spawn("sleep", &cfg).await.unwrap();
        let err = client.initialize(1).await.unwrap_err();
        assert!(matches!(err, McpError::Timeout(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn early_exit_surfaces_stderr_tail() {
        let cfg = echo_python_cfg(
            r#"
import sys
print("boot failed: missing config", file=sys.stderr, flush=True)
sys.exit(1)
"#,
        );
        let mut client = StdioClient::spawn("crashy", &cfg).await.unwrap();
        let err = client.initialize(5).await.unwrap_err();
        match err {
            McpError::EarlyExit { name, stderr_tail } => {
                assert_eq!(name, "crashy");
                assert!(
                    stderr_tail.contains("boot failed"),
                    "stderr tail missing message: {stderr_tail:?}"
                );
            }
            other => panic!("expected EarlyExit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unsupported_transport_rejected() {
        let cfg = McpServerConfig {
            transport: "sse".into(),
            command: "x".into(),
            args: vec![],
            env: BTreeMap::new(),
            timeouts: BTreeMap::new(),
        };
        let err = match StdioClient::spawn("x", &cfg).await {
            Ok(_) => panic!("expected Transport error for unsupported transport"),
            Err(e) => e,
        };
        assert!(matches!(err, McpError::Transport(_)));
    }

    #[tokio::test]
    async fn server_error_surfaced_structurally() {
        let cfg = echo_python_cfg(
            r#"
import json, sys
line = sys.stdin.readline()
req = json.loads(line)
resp = {"jsonrpc": "2.0", "id": req["id"], "error": {"code": -32601, "message": "Method not found"}}
print(json.dumps(resp), flush=True)
"#,
        );
        let mut client = StdioClient::spawn("err", &cfg).await.unwrap();
        let err = client.initialize(5).await.unwrap_err();
        match err {
            McpError::ServerError { code, message } => {
                assert_eq!(code, -32601);
                assert_eq!(message, "Method not found");
            }
            other => panic!("expected ServerError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn line_size_cap_rejects_runaway_response() {
        // Server writes >4MiB before newline.
        let cfg = echo_python_cfg(
            r#"
import sys
sys.stdin.readline()
sys.stdout.write("x" * (5 * 1024 * 1024))
sys.stdout.flush()
# Never sends newline; client should reject.
"#,
        );
        let mut client = StdioClient::spawn("flood", &cfg).await.unwrap();
        let err = client.initialize(10).await.unwrap_err();
        assert!(
            matches!(err, McpError::ProtocolViolation(_)),
            "expected ProtocolViolation, got {err:?}"
        );
    }
}
