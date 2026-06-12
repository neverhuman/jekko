//! Data structures describing `generated/sandbox-lanes.toml`.
//!
//! Pulled into a dedicated module so the validator + facade can stay focused
//! on logic rather than type definitions. Re-exported through
//! `crate::spec::*`.

use serde::{Deserialize, Serialize};

/// Top-level document mirroring `generated/sandbox-lanes.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanesDoc {
    pub schema_version: String,
    #[serde(default = "default_sandbox_root")]
    pub sandbox_root: String,
    #[serde(rename = "lane", default)]
    pub lanes: Vec<Lane>,
}

fn default_sandbox_root() -> String {
    "~/.local/share/agent-sandboxes".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lane {
    pub name: String,
    pub command_id: String,
    pub kind: LaneKind,
    pub purpose: String,
    pub command: String,
    pub cost: u32,
    #[serde(default)]
    pub destructive: bool,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub requires_network: bool,
    #[serde(default)]
    pub rules_covered: Vec<String>,
    #[serde(default)]
    pub required_artifacts: Vec<String>,

    pub workspace: WorkspaceCfg,
    pub runtime: RuntimeCfg,
    pub commands: CommandsCfg,
    pub environment: EnvCfg,
    #[serde(default)]
    pub feedback: FeedbackCfg,
    pub export: ExportCfg,
    #[serde(default)]
    pub cleanup: CleanupCfg,
    #[serde(default)]
    pub success: SuccessCfg,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LaneKind {
    Sandbox,
    Validation,
    Audit,
    Security,
    Release,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCfg {
    pub kind: WorkspaceKind,
    #[serde(default = "default_branch")]
    pub base_branch: String,
    #[serde(default = "default_branch_template")]
    pub branch_template: String,
}

fn default_branch() -> String {
    "main".into()
}
fn default_branch_template() -> String {
    "sandbox/{run_id}".into()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceKind {
    Worktree,
    Clone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCfg {
    pub backend: Backend,
    #[serde(default = "default_network")]
    pub network: NetworkPolicy,
    #[serde(default = "default_memory")]
    pub memory_limit: String,
    #[serde(default = "default_cpu")]
    pub cpu_limit: String,
    pub timeout_seconds: u64,
    /// Only used by docker/podman backends.
    #[serde(default)]
    pub image: Option<String>,
}

fn default_network() -> NetworkPolicy {
    NetworkPolicy::None
}
fn default_memory() -> String {
    "2GB".into()
}
fn default_cpu() -> String {
    "2".into()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Worktree,
    Bubblewrap,
    Docker,
    Podman,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SandboxNetworkPolicy {
    None,
    Bridge,
    Host,
}

pub use SandboxNetworkPolicy as NetworkPolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandsCfg {
    pub allowed_patterns: Vec<String>,
    #[serde(default)]
    pub denied_patterns: Vec<String>,
    pub wrapper: String,
    #[serde(default)]
    pub allowed_env: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvCfg {
    pub home: String,
    pub tmpdir: String,
    pub cache_home: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackCfg {
    #[serde(default = "yes")]
    pub capture_stdout: bool,
    #[serde(default = "yes")]
    pub capture_stderr: bool,
    #[serde(default = "yes")]
    pub capture_exit_code: bool,
    #[serde(default = "default_tail")]
    pub tail_lines: u32,
}

impl Default for FeedbackCfg {
    fn default() -> Self {
        Self {
            capture_stdout: true,
            capture_stderr: true,
            capture_exit_code: true,
            tail_lines: 200,
        }
    }
}

fn yes() -> bool {
    true
}
fn default_tail() -> u32 {
    200
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportCfg {
    pub patch_path: String,
    #[serde(default)]
    pub artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupCfg {
    #[serde(default = "yes")]
    pub auto_remove: bool,
    #[serde(default = "yes")]
    pub preserve_logs: bool,
    #[serde(default = "yes")]
    pub preserve_on_failure: bool,
}

impl Default for CleanupCfg {
    fn default() -> Self {
        Self {
            auto_remove: true,
            preserve_logs: true,
            preserve_on_failure: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SuccessCfg {
    #[serde(default)]
    pub exit_code_expected: Option<i32>,
    #[serde(default)]
    pub changed_files_max: Option<u32>,
    #[serde(default)]
    pub required_patch_present: bool,
}
