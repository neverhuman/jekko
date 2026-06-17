//! JSON-RPC 2.0 protocol types for MCP.
//!
//! Newline-delimited JSON framing per the MCP stdio transport (spec
//! 2024-11-05). Encoding is `serde_json::to_vec` + `b'\n'`; decoding is one
//! line at a time via [`decode_response`].

use serde::{Deserialize, Serialize};

use super::error::{McpError, McpResult};

/// Current JSON-RPC version. MCP uses 2.0.
pub const JSONRPC_VERSION: &str = "2.0";

/// One MCP JSON-RPC request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    /// JSON-RPC version. Must be `"2.0"`.
    pub jsonrpc: String,
    /// Request id (string in MCP).
    pub id: String,
    /// Method name (e.g. `"tools/list"`).
    pub method: String,
    /// Free-form params.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// One MCP JSON-RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    /// JSON-RPC version.
    pub jsonrpc: String,
    /// Request id (echo of the request that produced this response).
    pub id: String,
    /// Result, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorEnvelope>,
}

/// JSON-RPC error envelope. Surfaces as [`McpError::ServerError`] when a
/// response carries `error` instead of `result`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpErrorEnvelope {
    /// Numeric error code per JSON-RPC.
    pub code: i64,
    /// Short message.
    pub message: String,
    /// Optional structured data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Construct a new request envelope.
pub fn request(
    id: impl Into<String>,
    method: impl Into<String>,
    params: serde_json::Value,
) -> McpRequest {
    McpRequest {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: id.into(),
        method: method.into(),
        params,
    }
}

/// Encode a request as newline-delimited JSON (the MCP stdio framing).
pub fn encode_request(req: &McpRequest) -> McpResult<Vec<u8>> {
    let mut bytes =
        serde_json::to_vec(req).map_err(|e| McpError::ProtocolViolation(e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Decode a response from one newline-delimited JSON record.
pub fn decode_response(line: &str) -> McpResult<McpResponse> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.is_empty() {
        return Err(McpError::ProtocolViolation("empty response line".into()));
    }
    let resp: McpResponse =
        serde_json::from_str(line).map_err(|e| McpError::ProtocolViolation(e.to_string()))?;
    if resp.jsonrpc != JSONRPC_VERSION {
        return Err(McpError::ProtocolViolation(format!(
            "unexpected jsonrpc version: {}",
            resp.jsonrpc
        )));
    }
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let req = request("1", "tools/list", serde_json::json!({}));
        let bytes = encode_request(&req).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap().trim_end();
        let decoded: McpRequest = serde_json::from_str(text).unwrap();
        assert_eq!(decoded.method, "tools/list");
        assert_eq!(decoded.id, "1");
        assert_eq!(decoded.jsonrpc, JSONRPC_VERSION);
    }

    #[test]
    fn request_ends_with_newline() {
        let req = request("1", "tools/list", serde_json::json!({}));
        let bytes = encode_request(&req).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
    }

    #[test]
    fn response_round_trip_result() {
        let line = r#"{"jsonrpc":"2.0","id":"1","result":{"tools":[]}}"#;
        let resp = decode_response(line).unwrap();
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn response_round_trip_error() {
        let line =
            r#"{"jsonrpc":"2.0","id":"1","error":{"code":-32601,"message":"Method not found"}}"#;
        let resp = decode_response(line).unwrap();
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
    }

    #[test]
    fn response_strips_trailing_newline() {
        let line = "{\"jsonrpc\":\"2.0\",\"id\":\"1\",\"result\":{}}\n";
        let resp = decode_response(line).unwrap();
        assert!(resp.result.is_some());
    }

    #[test]
    fn response_rejects_wrong_jsonrpc_version() {
        let line = r#"{"jsonrpc":"1.0","id":"1","result":{}}"#;
        let err = decode_response(line).unwrap_err();
        assert!(matches!(err, McpError::ProtocolViolation(_)));
    }

    #[test]
    fn response_rejects_empty_line() {
        let err = decode_response("").unwrap_err();
        assert!(matches!(err, McpError::ProtocolViolation(_)));
        let err = decode_response("\n").unwrap_err();
        assert!(matches!(err, McpError::ProtocolViolation(_)));
    }

    #[test]
    fn response_rejects_invalid_json() {
        let err = decode_response("not json").unwrap_err();
        assert!(matches!(err, McpError::ProtocolViolation(_)));
    }
}
