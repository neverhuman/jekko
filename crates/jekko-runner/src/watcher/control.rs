//! Breach → control bridge (M11-cont).
//!
//! When a watcher breach fires, it emits a typed [`ControlTrigger`] over a
//! PRE-DECLARED control edge (the action + target are declared in the IR, never
//! invented at runtime). The trigger is bounded + secret-free so its event line
//! stays ≤512 B; the full receipt lands on disk.

use serde::{Deserialize, Serialize};

/// The pre-declared control action a breach may take. Each must correspond to a
/// control edge declared in the FlowGraph IR — the runtime can only fire actions
/// the author authorized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    Pause,
    Escalate,
    Branch,
    Kill,
    Quarantine,
    Reroute,
    Widen,
    RequireHuman,
}

impl ControlAction {
    pub fn as_str(self) -> &'static str {
        match self {
            ControlAction::Pause => "pause",
            ControlAction::Escalate => "escalate",
            ControlAction::Branch => "branch",
            ControlAction::Kill => "kill",
            ControlAction::Quarantine => "quarantine",
            ControlAction::Reroute => "reroute",
            ControlAction::Widen => "widen",
            ControlAction::RequireHuman => "require_human",
        }
    }
}

/// A typed control trigger emitted by a fired watcher breach. Bounded + secret-
/// free; serialized as the `ControlTrigger` event `data`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlTrigger {
    pub watcher_id: String,
    pub action: ControlAction,
    /// The node/group the action targets (a pre-declared control-edge endpoint).
    pub target: String,
}

/// Build a control trigger for a fired breach over a pre-declared edge.
pub fn make_control_trigger(
    watcher_id: &str,
    action: ControlAction,
    target: &str,
) -> ControlTrigger {
    ControlTrigger {
        watcher_id: watcher_id.to_string(),
        action,
        target: target.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_trigger_is_bounded_and_serializes_snake_case() {
        let t = make_control_trigger("convergence", ControlAction::Widen, "hero");
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"action\":\"widen\""));
        assert!(
            json.len() <= 512,
            "control trigger must fit the event budget"
        );
        assert_eq!(ControlAction::RequireHuman.as_str(), "require_human");
    }
}
