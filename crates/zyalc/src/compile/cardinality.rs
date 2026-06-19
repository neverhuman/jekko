//! Dynamic cardinality algebra (F1b).
//!
//! A fan-out / spawn group's instance count may be `static|config|source|expr|
//! discovered|adaptive`. Static/config (and source-with-estimate) expand to
//! concrete child boxes at compile time; the dynamic modes compile to a
//! `group.dynamic` placeholder carrying bounds + `runtime_state: "pending"`,
//! which a runtime `GraphPatched::ExpandDynamicGroup` later resolves.
//!
//! Bounded-by-construction is a hard rule: `discovered`/`adaptive` MUST declare
//! a `max` (else the live graph could grow without limit), and `discovered`
//! instances MUST have a stable identity (or an explicit `stable_index`
//! fallback) so replay is deterministic.

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum CardinalityMode {
    Static,
    Config,
    Source,
    Expr,
    Discovered,
    Adaptive,
}

impl CardinalityMode {
    fn parse(s: &str) -> Self {
        match s {
            "config" => Self::Config,
            "source" => Self::Source,
            "expr" => Self::Expr,
            "discovered" => Self::Discovered,
            "adaptive" => Self::Adaptive,
            _ => Self::Static,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Config => "config",
            Self::Source => "source",
            Self::Expr => "expr",
            Self::Discovered => "discovered",
            Self::Adaptive => "adaptive",
        }
    }

    /// `discovered`/`adaptive` grow the graph at runtime and must be bounded.
    fn must_be_bounded(self) -> bool {
        matches!(self, Self::Discovered | Self::Adaptive)
    }
}

#[derive(Debug, Clone)]
pub(super) struct CardinalitySpec {
    pub mode: CardinalityMode,
    pub value: Option<u64>,
    pub min: Option<u64>,
    pub max: Option<u64>,
    pub identity: Option<String>,
    pub identity_fallback: Option<String>,
}

/// Parse a cardinality block (bare integer or object). `mode` is read from the
/// new `mode` key, falling back to the legacy `kind` key.
pub(super) fn parse_cardinality(card: &Value) -> CardinalitySpec {
    if let Some(n) = card.as_u64() {
        return CardinalitySpec {
            mode: CardinalityMode::Static,
            value: Some(n),
            min: Some(n),
            max: Some(n),
            identity: None,
            identity_fallback: None,
        };
    }
    let o = card.as_object();
    let s = |k: &str| o.and_then(|m| m.get(k)).and_then(Value::as_str);
    let u = |k: &str| o.and_then(|m| m.get(k)).and_then(Value::as_u64);
    // `mode` (or the legacy `kind` alias); absent → the static default.
    let mode = CardinalityMode::parse(s("mode").or_else(|| s("kind")).unwrap_or("static"));
    let value = u("value").or_else(|| {
        o.and_then(|m| m.get("items"))
            .and_then(Value::as_array)
            .map(|a| a.len() as u64)
    });
    CardinalitySpec {
        mode,
        value,
        min: u("min"),
        max: u("max"),
        identity: s("identity").map(str::to_string),
        identity_fallback: s("identity_fallback").map(str::to_string),
    }
}

impl CardinalitySpec {
    /// Whether the instance count is known statically (static/config with a
    /// value). The single source of truth shared by `flowgraph::count_of`.
    pub(super) fn resolved(&self) -> bool {
        matches!(self.mode, CardinalityMode::Static | CardinalityMode::Config)
            && self.value.is_some()
    }

    /// The formal cardinality value stamped onto a `group.dynamic` placeholder.
    pub(super) fn placeholder_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("mode".into(), json!(self.mode.as_str()));
        if let Some(v) = self.value {
            m.insert("value".into(), json!(v));
        }
        if let Some(v) = self.min {
            m.insert("min".into(), json!(v));
        }
        if let Some(v) = self.max {
            m.insert("max".into(), json!(v));
        }
        if let Some(id) = &self.identity {
            m.insert("identity".into(), json!(id));
        }
        if let Some(fb) = &self.identity_fallback {
            m.insert("identity_fallback".into(), json!(fb));
        }
        m.insert("runtime_state".into(), json!("pending"));
        Value::Object(m)
    }
}

/// Validate dynamic-cardinality bounds + identity across all authored nodes.
/// No-op on synthesized graphs and on static/config/source cardinalities.
pub(super) fn validate_cardinality(root: &Map<String, Value>) -> Result<()> {
    let Some(nodes) = root
        .get("graph")
        .and_then(Value::as_object)
        .and_then(|g| g.get("nodes"))
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    for n in nodes {
        let Some(nm) = n.as_object() else { continue };
        let id = nm.get("id").and_then(Value::as_str).unwrap_or("<node>");
        let card = nm.get("cardinality").or_else(|| {
            nm.get("spawn")
                .and_then(Value::as_object)
                .and_then(|s| s.get("cardinality"))
        });
        let Some(card) = card else { continue };
        let spec = parse_cardinality(card);
        if spec.mode.must_be_bounded() && spec.max.is_none() {
            return Err(anyhow!(
                "ZYAL_E_DYNAMIC_CARDINALITY_UNBOUNDED: node `{id}` has `{}` cardinality without a `max` bound (the live graph could grow without limit)",
                spec.mode.as_str()
            ));
        }
        if spec.mode == CardinalityMode::Discovered
            && spec.identity.is_none()
            && spec.identity_fallback.is_none()
        {
            return Err(anyhow!(
                "ZYAL_E_DYNAMIC_CARDINALITY_NO_IDENTITY: discovered node `{id}` needs an `identity` (or `identity_fallback: stable_index`) for replay determinism"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(card: Value) -> Map<String, Value> {
        json!({ "graph": { "nodes": [ { "id": "g", "type": "fanout", "cardinality": card } ] } })
            .as_object()
            .unwrap()
            .clone()
    }

    #[test]
    fn static_and_config_are_always_ok() {
        validate_cardinality(&node(json!({ "kind": "static", "value": 4 }))).unwrap();
        validate_cardinality(&node(json!({ "mode": "config", "value": 7 }))).unwrap();
        validate_cardinality(&node(json!(3))).unwrap();
    }

    #[test]
    fn discovered_without_max_is_unbounded() {
        let err = validate_cardinality(&node(json!({ "mode": "discovered", "identity": "url" })))
            .unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_DYNAMIC_CARDINALITY_UNBOUNDED"),
            "got {err}"
        );
    }

    #[test]
    fn adaptive_without_max_is_unbounded() {
        let err = validate_cardinality(&node(json!({ "mode": "adaptive" }))).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_DYNAMIC_CARDINALITY_UNBOUNDED"),
            "got {err}"
        );
    }

    #[test]
    fn discovered_without_identity_is_rejected() {
        let err =
            validate_cardinality(&node(json!({ "mode": "discovered", "max": 16 }))).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_DYNAMIC_CARDINALITY_NO_IDENTITY"),
            "got {err}"
        );
    }

    #[test]
    fn bounded_discovered_with_identity_compiles() {
        validate_cardinality(&node(
            json!({ "mode": "discovered", "max": 16, "identity": "url" }),
        ))
        .unwrap();
        // stable_index fallback is also accepted
        validate_cardinality(&node(
            json!({ "mode": "discovered", "max": 8, "identity_fallback": "stable_index" }),
        ))
        .unwrap();
    }

    #[test]
    fn placeholder_value_carries_bounds_and_pending_state() {
        let spec =
            parse_cardinality(&json!({ "mode": "discovered", "max": 16, "identity": "url" }));
        let v = spec.placeholder_value();
        assert_eq!(v["mode"], "discovered");
        assert_eq!(v["max"], 16);
        assert_eq!(v["identity"], "url");
        assert_eq!(v["runtime_state"], "pending");
    }
}
