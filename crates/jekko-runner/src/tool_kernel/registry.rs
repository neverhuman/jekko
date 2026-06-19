use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zyal_core::Capability;

use super::ToolLease;

/// How a tool is implemented. The executable registry (by `tool_id`) is separate
/// from the visual `node-types.json` registry; a node binds both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Mcp,
    Shell,
    Code,
    Http,
    Workflow,
    Builtin,
    Plugin,
}

/// An executable tool the kernel can run. Parsed from a `tools:` block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub tool_id: String,
    pub kind: ToolKind,
    /// The visual node kind this tool binds to (`node-types.json`).
    pub node_type: String,
    pub version: String,
    pub side_effecting: bool,
    pub capabilities: Vec<Capability>,
    pub sandbox_profile: String,
    /// Deterministic tools are safe to cache + replay.
    pub deterministic: bool,
}

impl ToolDescriptor {
    pub(crate) fn from_value(tool_id: &str, v: &Value) -> Self {
        let o = v.as_object();
        let get_str = |k: &str| o.and_then(|m| m.get(k)).and_then(Value::as_str);
        let kind = match get_str("kind").unwrap_or("builtin") {
            "mcp" => ToolKind::Mcp,
            "shell" => ToolKind::Shell,
            "code" => ToolKind::Code,
            "http" => ToolKind::Http,
            "workflow" => ToolKind::Workflow,
            "plugin" => ToolKind::Plugin,
            _ => ToolKind::Builtin,
        };
        let capabilities = o
            .and_then(|m| m.get("capabilities"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|c| serde_json::from_value::<Capability>(c.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        ToolDescriptor {
            tool_id: tool_id.to_string(),
            kind,
            node_type: get_str("node_type").unwrap_or("tool").to_string(),
            version: get_str("version").unwrap_or("1").to_string(),
            side_effecting: o
                .and_then(|m| m.get("side_effecting"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
            capabilities,
            sandbox_profile: get_str("sandbox").unwrap_or("sealed").to_string(),
            deterministic: o
                .and_then(|m| m.get("deterministic"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }
}

/// The executable tool catalog (by `tool_id`). Deterministically ordered.
#[derive(Debug, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, ToolDescriptor>,
}

impl ToolRegistry {
    /// Build from a `tools:` block (a map of `tool_id` → descriptor).
    pub fn from_block(block: &Value) -> Self {
        let mut tools = BTreeMap::new();
        if let Some(map) = block.as_object() {
            for (id, spec) in map {
                tools.insert(id.clone(), ToolDescriptor::from_value(id, spec));
            }
        }
        ToolRegistry { tools }
    }

    pub fn lookup(&self, tool_id: &str) -> Option<&ToolDescriptor> {
        self.tools.get(tool_id)
    }

    pub fn ids(&self) -> Vec<&str> {
        self.tools.keys().map(String::as_str).collect()
    }
}

/// The result of a tool invocation (secret-free: `output` is scanned by the
/// receipt finalizer before it can persist).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub output: String,
    pub cost_usd: f64,
    pub latency_ms: u64,
}

/// A pluggable tool implementation. The kernel never calls `invoke` without a
/// granted [`ToolLease`].
pub trait ToolAdapter {
    fn tool_id(&self) -> &str;
    fn invoke(&self, lease: &ToolLease, input: &Value) -> Result<ToolOutput, String>;
}
