//! Per-role tool-use policy for orchestrator model calls.
//!
//! The `jekko-runtime` agent executor exposes a rich tool registry (bash,
//! read, write, edit, glob, grep, webfetch, websearch, task). For
//! orchestrator-spawned model calls, we want each role's surface area to
//! match what it actually needs:
//!
//! - **Off** — pure text reasoning. Frame / Critique / Reducer-class roles
//!   that synthesize from already-loaded evidence; tools would only invite
//!   sprawl. Sets `JEKKO_RUN_DISABLE_TOOLS=1`.
//! - **ReadOnly** — can read code and search externally but cannot mutate.
//!   Brainstorm / Verifier / Parity-author / Memory-curate roles that need
//!   to explore the repo and the web. Sets `JEKKO_RUN_TOOL_ALLOWLIST` to
//!   `read,grep,glob,webfetch,websearch,task`.
//! - **Full** — can also write, edit, and run shells. Implement / Healing /
//!   StuckDebug roles that produce code or run gap-closure commands. No env
//!   restriction; uses jekko-runtime's default tool set.
//!
//! Phase D wires this via `JekkoRuntimeModelClient::complete()` — the
//! `ModelClient` trait signature stays unchanged because the mode is
//! derived from the task kind, not passed in.

use crate::model_policy::ModelTaskKind;

/// Allowed tool surface for one model call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolMode {
    /// Tools disabled. Sets `JEKKO_RUN_DISABLE_TOOLS=1`.
    #[default]
    Off,
    /// Read-only tools (read, grep, glob, webfetch, websearch, task). Sets
    /// `JEKKO_RUN_TOOL_ALLOWLIST` to that list.
    ReadOnly,
    /// Full tool surface (read + write + edit + bash + …). No env restriction.
    Full,
}

impl ToolMode {
    /// Comma-separated allowlist string for `JEKKO_RUN_TOOL_ALLOWLIST`, or
    /// `None` when no allowlist applies (Off disables all tools; Full uses
    /// jekko-runtime's default).
    pub fn allowlist_env(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::ReadOnly => Some("read,grep,glob,webfetch,websearch,task"),
            Self::Full => None,
        }
    }

    /// Whether tools should be disabled entirely via
    /// `JEKKO_RUN_DISABLE_TOOLS=1`.
    pub fn disables_tools(self) -> bool {
        matches!(self, Self::Off)
    }
}

/// Default tool-use policy for each [`ModelTaskKind`].
///
/// The mapping reflects the spec's anti-sprawl posture: explore phases can
/// touch the repo and the web (ReadOnly); synthesize/reduce phases stay
/// text-only (Off); only implement/heal/debug roles get full mutation
/// surface (Full).
pub fn requires_tools(kind: ModelTaskKind) -> ToolMode {
    use ModelTaskKind::*;
    match kind {
        // Pure synthesis / framing / reducer roles — no tool access.
        Frame | StageCritique | StageReduce | PhaseFinalize | Critic | MetaJudge | JudgePatch
        | Review | KnowledgeCurate => ToolMode::Off,

        // Exploration / verification / curation roles — read-only surface.
        StageBrainstorm | PhaseBrainstorm | Hypothesis | Verifier | ParityGenerate
        | MemoryCurate | LiteratureSynthesis | RedTeam | HeroGenerate | PerfGap => {
            ToolMode::ReadOnly
        }

        // Producer / debugger / healer roles — full surface.
        Implement | Healing | StuckDebug | PerfClose | HardEscalation => ToolMode::Full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_disables_tools_and_has_no_allowlist() {
        assert!(ToolMode::Off.disables_tools());
        assert_eq!(ToolMode::Off.allowlist_env(), None);
    }

    #[test]
    fn readonly_exposes_safe_tools() {
        assert!(!ToolMode::ReadOnly.disables_tools());
        let allowlist = ToolMode::ReadOnly.allowlist_env().unwrap();
        for required in ["read", "grep", "glob", "webfetch", "websearch", "task"] {
            assert!(allowlist.contains(required), "missing tool: {required}");
        }
        // Explicitly excluded mutators:
        assert!(!allowlist.contains("write"));
        assert!(!allowlist.contains("edit"));
        assert!(!allowlist.contains("bash"));
    }

    #[test]
    fn full_uses_runtime_defaults() {
        assert!(!ToolMode::Full.disables_tools());
        assert_eq!(ToolMode::Full.allowlist_env(), None);
    }

    #[test]
    fn requires_tools_matches_spec_mapping() {
        assert_eq!(requires_tools(ModelTaskKind::Frame), ToolMode::Off);
        assert_eq!(requires_tools(ModelTaskKind::StageCritique), ToolMode::Off);
        assert_eq!(requires_tools(ModelTaskKind::StageReduce), ToolMode::Off);
        assert_eq!(
            requires_tools(ModelTaskKind::StageBrainstorm),
            ToolMode::ReadOnly
        );
        assert_eq!(requires_tools(ModelTaskKind::Verifier), ToolMode::ReadOnly);
        assert_eq!(
            requires_tools(ModelTaskKind::ParityGenerate),
            ToolMode::ReadOnly
        );
        assert_eq!(requires_tools(ModelTaskKind::Implement), ToolMode::Full);
        assert_eq!(requires_tools(ModelTaskKind::Healing), ToolMode::Full);
        assert_eq!(requires_tools(ModelTaskKind::StuckDebug), ToolMode::Full);
    }
}
