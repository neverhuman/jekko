//! Watcher runtime + breach detection (M11-cont).
//!
//! A watcher computes a value each observation (a [`zyal_core::zexpr`]
//! expression over named inputs) and fires a typed control trigger when its
//! breach predicate SUSTAINS for `for_n` observations — with hysteresis so a
//! noisy signal can't flap the control action.

use serde::{Deserialize, Serialize};
use zyal_core::zexpr::{eval_zexpr, CompiledZExpr, ZBindings, ZValue};

use super::control::{make_control_trigger, ControlAction, ControlTrigger};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatcherKind {
    Equation,
    Value,
    Plot,
    Algorithm,
}

/// A breach predicate over the watcher value, with debounce + hysteresis.
pub struct BreachSpec {
    /// Predicate evaluated each observation; may read `value` (the watcher value).
    pub when: CompiledZExpr,
    /// Fire only after `when` holds for this many CONSECUTIVE observations (≥1).
    pub for_n: u32,
    /// Once fired, require this many consecutive NON-breaching obs to re-arm (≥1).
    pub hysteresis: u32,
    pub action: ControlAction,
    pub target: String,
}

/// A live watcher: computes a value and fires a control trigger when its breach
/// predicate sustains. Pure over its observation stream (no clock, no I/O).
pub struct WatcherRuntime {
    pub watcher_id: String,
    pub kind: WatcherKind,
    /// The value expression (e.g. `score - prev_score`).
    pub expr: CompiledZExpr,
    pub breach: BreachSpec,
    consecutive_breach: u32,
    consecutive_clear: u32,
    fired: bool,
}

/// Overlays the computed watcher `value` onto an observation's bindings so the
/// breach predicate can reference it.
struct ValueOverlay<'a> {
    value: ZValue,
    inner: &'a dyn ZBindings,
}

impl ZBindings for ValueOverlay<'_> {
    fn resolve(&self, name: &str) -> Option<ZValue> {
        if name == "value" {
            return Some(self.value.clone());
        }
        self.inner.resolve(name)
    }
}

fn truthy(v: &ZValue) -> bool {
    match v {
        ZValue::Bool(b) => *b,
        ZValue::Num(n) => *n != 0.0 && !n.is_nan(),
        ZValue::Null => false,
        ZValue::Str(s) => !s.is_empty(),
        ZValue::Arr(a) => !a.is_empty(),
    }
}

impl WatcherRuntime {
    pub fn new(
        watcher_id: impl Into<String>,
        kind: WatcherKind,
        expr: CompiledZExpr,
        breach: BreachSpec,
    ) -> Self {
        Self {
            watcher_id: watcher_id.into(),
            kind,
            expr,
            breach,
            consecutive_breach: 0,
            consecutive_clear: 0,
            fired: false,
        }
    }

    /// The latest computed value (display / debugging).
    pub fn evaluate(&self, bindings: &dyn ZBindings) -> Result<ZValue, String> {
        eval_zexpr(&self.expr, bindings)
    }

    /// Feed one observation. Returns `Some(trigger)` EXACTLY when a breach fires
    /// (the predicate first sustains for `for_n` obs); sustained obs do not
    /// re-fire until a hysteresis-length clear re-arms the watcher.
    pub fn observe(&mut self, bindings: &dyn ZBindings) -> Result<Option<ControlTrigger>, String> {
        let value = eval_zexpr(&self.expr, bindings)?;
        let overlay = ValueOverlay {
            value,
            inner: bindings,
        };
        let breached = truthy(&eval_zexpr(&self.breach.when, &overlay)?);
        if breached {
            self.consecutive_clear = 0;
            self.consecutive_breach = self.consecutive_breach.saturating_add(1);
            if !self.fired && self.consecutive_breach >= self.breach.for_n.max(1) {
                self.fired = true;
                return Ok(Some(make_control_trigger(
                    &self.watcher_id,
                    self.breach.action,
                    &self.breach.target,
                )));
            }
        } else {
            self.consecutive_breach = 0;
            self.consecutive_clear = self.consecutive_clear.saturating_add(1);
            if self.fired && self.consecutive_clear >= self.breach.hysteresis.max(1) {
                self.fired = false; // re-arm after a sustained clear
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zyal_core::zexpr::compile;

    struct Vars(HashMap<String, f64>);
    impl ZBindings for Vars {
        fn resolve(&self, name: &str) -> Option<ZValue> {
            self.0.get(name).copied().map(ZValue::Num)
        }
    }
    fn obs(score: f64) -> Vars {
        let mut m = HashMap::new();
        m.insert("score".to_string(), score);
        Vars(m)
    }

    fn watcher(for_n: u32, hysteresis: u32) -> WatcherRuntime {
        WatcherRuntime::new(
            "w",
            WatcherKind::Equation,
            compile("score").unwrap(),
            BreachSpec {
                when: compile("value > 0.9").unwrap(),
                for_n,
                hysteresis,
                action: ControlAction::Widen,
                target: "hero".to_string(),
            },
        )
    }

    #[test]
    fn breach_fires_only_after_for_n_consecutive_then_holds() {
        let mut w = watcher(3, 1);
        assert!(w.observe(&obs(0.95)).unwrap().is_none()); // 1
        assert!(w.observe(&obs(0.95)).unwrap().is_none()); // 2
        let t = w.observe(&obs(0.95)).unwrap().unwrap(); // 3 → fire
        assert_eq!(t.action, ControlAction::Widen);
        assert_eq!(t.target, "hero");
        // sustained breach does NOT re-fire
        assert!(w.observe(&obs(0.95)).unwrap().is_none());
    }

    #[test]
    fn a_clear_observation_resets_the_consecutive_count() {
        let mut w = watcher(3, 1);
        w.observe(&obs(0.95)).unwrap();
        w.observe(&obs(0.50)).unwrap(); // clear → reset the streak
        w.observe(&obs(0.95)).unwrap();
        w.observe(&obs(0.95)).unwrap();
        // only 2 consecutive since the reset → still no fire
        assert!(w.observe(&obs(0.50)).unwrap().is_none());
    }

    #[test]
    fn hysteresis_rearms_only_after_a_sustained_clear() {
        let mut w = watcher(1, 2); // fire immediately; re-arm after 2 clears
        assert!(w.observe(&obs(0.95)).unwrap().is_some()); // fire
        assert!(w.observe(&obs(0.50)).unwrap().is_none()); // clear 1 (not re-armed)
        assert!(w.observe(&obs(0.50)).unwrap().is_none()); // clear 2 → re-armed
        assert!(w.observe(&obs(0.95)).unwrap().is_some()); // fires again
    }
}
