//! Source runtime (M12) — every external value enters as a typed, provenance +
//! taint-tagged [`ValueEnvelope`]. `data_feeds` poll external sources; the
//! runtime validates + wraps + gates them and emits a bounded `feed_tick` frame
//! (≤512 bytes; the full envelope is written to storage). Transitive taint
//! flows along edges so a downstream `decide_arm` gate is structural, not
//! advisory (law #6).
//!
//! This module owns the spec types, envelope construction, the ≤512 B frame, a
//! deterministic fixture provider (CI / replay; no network), the deny-by-default
//! connectors registry, and the taint transfer function. The real network
//! pollers (websocket / stock APIs) + shared-memory execution land in M12-cont.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zyal_core::{transfer_taint, ProvenanceRef, TaintLabel, TaintSet, ValueEnvelope};

use crate::hashing::sha256_hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedKind {
    StockTicker,
    OnlineData,
    Websocket,
    FileTail,
}

impl FeedKind {
    fn parse(s: &str) -> Self {
        match s {
            "online_data" => FeedKind::OnlineData,
            "websocket" => FeedKind::Websocket,
            "file_tail" => FeedKind::FileTail,
            _ => FeedKind::StockTicker,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            FeedKind::StockTicker => "stock_ticker",
            FeedKind::OnlineData => "online_data",
            FeedKind::Websocket => "websocket",
            FeedKind::FileTail => "file_tail",
        }
    }
}

/// A declared data feed. External feeds default to `web_content` taint (a
/// `file_tail` from the local run is trusted) and a `value` primary field.
#[derive(Debug, Clone)]
pub struct DataFeedSpec {
    pub id: String,
    pub kind: FeedKind,
    pub source: String,
    pub symbols: Vec<String>,
    pub value_schema_id: String,
    pub primary_field: String,
    pub poll_interval_ms: u64,
    pub taint: TaintSet,
}

impl DataFeedSpec {
    pub fn from_value(id: &str, v: &Value) -> Self {
        let o = v.as_object();
        let s = |k: &str| o.and_then(|m| m.get(k)).and_then(Value::as_str);
        let kind = FeedKind::parse(s("kind").unwrap_or("stock_ticker"));
        let symbols = o
            .and_then(|m| m.get("symbols"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        // Fail-closed: EVERY feed is `web_content` (untrusted) unless the operator
        // explicitly declares `trust: trusted` — a feed source (even a local file
        // tail, which could be attacker-writable) is never trusted by default.
        let taint = if s("trust") == Some("trusted") {
            TaintSet::clean()
        } else {
            TaintSet::from_labels([TaintLabel::WebContent])
        };
        DataFeedSpec {
            id: id.to_string(),
            kind,
            source: s("source").unwrap_or("").to_string(),
            symbols,
            value_schema_id: s("value_schema").unwrap_or("scalar").to_string(),
            primary_field: s("primary").unwrap_or("value").to_string(),
            // Accept either a numeric `poll_interval_ms` or the compiled IR's
            // duration string `poll_interval` (e.g. "15s", "500ms", "1m"); the IR
            // emits the latter, so a string must not silently fall back to default.
            poll_interval_ms: o
                .and_then(|m| m.get("poll_interval_ms"))
                .and_then(Value::as_u64)
                .or_else(|| s("poll_interval").and_then(parse_duration_ms))
                .unwrap_or(15_000),
            taint,
        }
    }
}

/// Parse a duration string (`"500ms"`, `"15s"`, `"2m"`, `"1h"`, or a bare number
/// of ms) into milliseconds. Returns `None` on an unparseable value so the caller
/// can fall back to its default. Pure + deterministic.
pub fn parse_duration_ms(s: &str) -> Option<u64> {
    let t = s.trim();
    if let Ok(n) = t.parse::<u64>() {
        return Some(n); // bare number = milliseconds
    }
    let (num, mult): (&str, u64) = if let Some(n) = t.strip_suffix("ms") {
        (n, 1)
    } else if let Some(n) = t.strip_suffix('s') {
        (n, 1_000)
    } else if let Some(n) = t.strip_suffix('m') {
        (n, 60_000)
    } else if let Some(n) = t.strip_suffix('h') {
        (n, 3_600_000)
    } else {
        return None;
    };
    num.trim().parse::<u64>().ok().map(|n| n * mult)
}

/// Wrap a raw tick into a provenance + taint-tagged [`ValueEnvelope`]. The
/// runner (not a model) computes the content hash.
pub fn envelope_from_tick(
    spec: &DataFeedSpec,
    run_id: &str,
    seq: u64,
    value: Value,
    observed_at_ms: i64,
) -> ValueEnvelope {
    // Canonicalize (recursively sort object keys) before hashing so the content
    // hash is byte-stable regardless of field order / serde_json features (law #2).
    let bytes = serde_json::to_vec(&canonicalize_json(&value)).unwrap_or_default();
    let content_hash = format!("sha256:{}", sha256_hex(&bytes));
    let primary_value = value
        .get(&spec.primary_field)
        .and_then(Value::as_f64)
        .or_else(|| value.as_f64());
    ValueEnvelope {
        id: format!("{}:{seq}", spec.id),
        run_id: run_id.to_string(),
        source_node_id: spec.id.clone(),
        seq,
        primary_value,
        value,
        value_schema_id: spec.value_schema_id.clone(),
        observed_at_ms,
        source_ts_ms: None,
        latency_ms: None,
        freshness_ms: None,
        provenance: ProvenanceRef {
            id: format!("prov:{}:{seq}", spec.id),
            kind: spec.kind.as_str().to_string(),
            // never store a raw source URL: strip `user:pass@` userinfo + the query
            // string (which can carry api keys) before it persists.
            source: redact_source(&spec.source),
            uri_redacted: None,
            content_hash,
            receipt_path: String::new(),
        },
        taint: spec.taint.clone(),
    }
}

fn taint_labels(t: &TaintSet) -> Vec<String> {
    t.labels
        .iter()
        .filter_map(|l| serde_json::to_value(l).ok())
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// Recursively sort object keys so serialization is byte-stable for hashing.
fn canonicalize_json(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonicalize_json(&m[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(a) => Value::Array(a.iter().map(canonicalize_json).collect()),
        other => other.clone(),
    }
}

/// Strip `user:pass@` userinfo and the query string from a source URL so a
/// credential embedded in a feed source can never persist in provenance.
fn redact_source(s: &str) -> String {
    let no_query = s.split('?').next().unwrap_or(s);
    if let Some(scheme_end) = no_query.find("://") {
        let after = &no_query[scheme_end + 3..];
        if let Some(at) = after.find('@') {
            return format!(
                "{}://<redacted>@{}",
                &no_query[..scheme_end],
                &after[at + 1..]
            );
        }
    }
    no_query.to_string()
}

/// A bounded `feed_tick` frame (ids + the scalar latest value + taint labels +
/// the provenance hash). The full envelope lives in storage. Kept ≤512 bytes.
pub fn feed_tick_frame(env: &ValueEnvelope, label: &str) -> Value {
    // bound the label so the frame stays within the ≤512 B budget the EventSink
    // hard-enforces at emit time.
    let label: String = label.chars().take(80).collect();
    json!({
        "feed": {
            "id": env.source_node_id,
            "label": label,
            "seq": env.seq,
            "latest_value": env.primary_value,
        },
        "taint": taint_labels(&env.taint),
        "prov": env.provenance.content_hash,
    })
}

/// A deterministic fixture source for CI / replay — no network. The tick is a
/// pure function of `(feed id, seq)` so a run is reproducible.
#[derive(Debug, Default)]
pub struct FixtureSourceProvider;

impl FixtureSourceProvider {
    pub fn tick(&self, spec: &DataFeedSpec, seq: u64) -> Value {
        // deterministic pseudo-value seeded by the feed id + seq (no randomness)
        let seed = sha256_hex(format!("{}:{seq}", spec.id).as_bytes());
        let n = u32::from_str_radix(&seed[..8], 16).unwrap_or(0);
        let value = (n % 10_000) as f64 / 100.0;
        // the feed's first declared symbol, or empty when it declares none (e.g. a file-tail feed)
        let symbol = spec.symbols.first().cloned().unwrap_or_default();
        json!({ "value": value, "symbol": symbol })
    }
}

// ---- connectors registry (deny-by-default) ---------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorKind {
    Database,
    Api,
    Queue,
    Filesystem,
}

/// An external connector. Capabilities are DENY-BY-DEFAULT: a capability must be
/// explicitly granted, and egress must match the allowlist.
#[derive(Debug, Clone)]
pub struct ConnectorSpec {
    pub id: String,
    pub kind: ConnectorKind,
    pub secret_ref: Option<String>,
    pub granted: Vec<String>,
    pub egress_allowlist: Vec<String>,
    pub rate_limit_per_min: u32,
    pub max_bytes: u64,
}

impl ConnectorSpec {
    pub fn from_value(id: &str, v: &Value) -> Self {
        let o = v.as_object();
        let arr = |k: &str| {
            o.and_then(|m| m.get(k))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        let kind = match o.and_then(|m| m.get("kind")).and_then(Value::as_str) {
            Some("api") => ConnectorKind::Api,
            Some("queue") => ConnectorKind::Queue,
            Some("filesystem") => ConnectorKind::Filesystem,
            _ => ConnectorKind::Database,
        };
        ConnectorSpec {
            id: id.to_string(),
            kind,
            secret_ref: o
                .and_then(|m| m.get("secret_ref"))
                .and_then(Value::as_str)
                .map(str::to_string),
            granted: arr("capabilities"),
            egress_allowlist: arr("egress_allowlist"),
            rate_limit_per_min: o
                .and_then(|m| m.get("rate_limit_per_min"))
                .and_then(Value::as_u64)
                .unwrap_or(60) as u32,
            max_bytes: o
                .and_then(|m| m.get("max_bytes"))
                .and_then(Value::as_u64)
                .unwrap_or(1_048_576),
        }
    }

    /// Deny-by-default: a capability is allowed only if explicitly granted.
    pub fn allows(&self, capability: &str) -> bool {
        self.granted.iter().any(|c| c == capability)
    }

    /// Egress is allowed only if the host matches the allowlist (empty = none).
    pub fn allows_egress(&self, host: &str) -> bool {
        self.egress_allowlist
            .iter()
            .any(|h| host == h || host.ends_with(&format!(".{h}")))
    }

    /// Fully authorize ONE connector call (M12-cont). Deny-by-default at every
    /// step: the capability must be granted, the host must match the egress
    /// allowlist, the call must be within the per-minute rate limit, and the
    /// payload within `max_bytes`. The first failing check denies.
    pub fn authorize_call(
        &self,
        capability: &str,
        host: &str,
        bytes: u64,
        calls_this_minute: u32,
    ) -> Result<(), ConnectorDenial> {
        if !self.allows(capability) {
            return Err(ConnectorDenial::CapabilityDenied);
        }
        if !self.allows_egress(host) {
            return Err(ConnectorDenial::EgressDenied);
        }
        if calls_this_minute >= self.rate_limit_per_min {
            return Err(ConnectorDenial::RateLimited);
        }
        if bytes > self.max_bytes {
            return Err(ConnectorDenial::SizeExceeded);
        }
        Ok(())
    }
}

/// Why a connector call was denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorDenial {
    CapabilityDenied,
    EgressDenied,
    RateLimited,
    SizeExceeded,
}

// ---- feed poller scheduling (M12-cont) -------------------------------------

const MAX_BACKOFF_MS: u64 = 5 * 60_000;

/// The delay before a feed's next poll. On the happy path this is the declared
/// `poll_interval_ms`; after `failures` consecutive errors it backs off ×2^k
/// (capped), plus bounded DETERMINISTIC jitter derived from `seed` (never a
/// clock/RNG, so replays match) — preventing thundering-herd reconnects.
pub fn next_poll_delay_ms(spec: &DataFeedSpec, failures: u32, seed: u64) -> u64 {
    let base = spec.poll_interval_ms.max(1);
    if failures == 0 {
        return base;
    }
    let factor = 1u64 << failures.min(10); // ×2^failures, capped at ×1024
    let backed = base.saturating_mul(factor).min(MAX_BACKOFF_MS);
    // jitter in [0, base/2], deterministic in `seed`.
    let jitter = seed.wrapping_mul(2_654_435_761) % (base / 2 + 1);
    backed.saturating_add(jitter).min(MAX_BACKOFF_MS)
}

// ---- shared memory bindings (M12-cont) -------------------------------------

/// The visibility scope of a shared-memory store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    Global,
    Run,
    Lane,
}

/// How a shared-memory store is retrieved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Retrieval {
    Key,
    Vector,
    Hybrid,
}

/// A per-binding access rule: which node may write/read, and which taint labels
/// it may carry through. A write whose envelope carries a label NOT in the
/// allowlist is rejected — tainted data can never silently enter durable memory
/// (law #6). Drives the `writes_memory`/`reads_memory` edges in the IR.
#[derive(Debug, Clone)]
pub struct MemoryBinding {
    pub node_id: String,
    pub allow_taint: Vec<TaintLabel>,
}

const ALL_TAINT_LABELS: [TaintLabel; 7] = [
    TaintLabel::Trusted,
    TaintLabel::WebContent,
    TaintLabel::ProdData,
    TaintLabel::ModelOutput,
    TaintLabel::Untrusted,
    TaintLabel::Secret,
    TaintLabel::Sanitized,
];

/// A taint-gated shared-memory store. Writers/readers are explicit bindings;
/// writes are taint-allowlisted per binding.
pub struct SharedMemory {
    pub scope: MemoryScope,
    pub retrieval: Retrieval,
    writers: Vec<MemoryBinding>,
    readers: Vec<MemoryBinding>,
    store: BTreeMap<String, ValueEnvelope>,
}

impl SharedMemory {
    pub fn new(
        scope: MemoryScope,
        retrieval: Retrieval,
        writers: Vec<MemoryBinding>,
        readers: Vec<MemoryBinding>,
    ) -> Self {
        Self {
            scope,
            retrieval,
            writers,
            readers,
            store: BTreeMap::new(),
        }
    }

    /// Write `value` at `key` from `node_id`. Rejected unless `node_id` is a
    /// declared writer AND every taint label on the value is in that writer's
    /// allowlist (so e.g. a `Secret`/`WebContent` value can't enter memory unless
    /// the binding explicitly permits it).
    pub fn write(&mut self, node_id: &str, key: &str, value: ValueEnvelope) -> Result<(), String> {
        let binding = self
            .writers
            .iter()
            .find(|w| w.node_id == node_id)
            .ok_or_else(|| format!("`{node_id}` is not a declared writer"))?;
        for &label in &ALL_TAINT_LABELS {
            if value.taint.has(label) && !binding.allow_taint.contains(&label) {
                return Err(format!(
                    "taint `{label:?}` is not allowed into memory by `{node_id}`"
                ));
            }
        }
        self.store.insert(key.to_string(), value);
        Ok(())
    }

    /// Read `key` for `node_id`. Rejected unless `node_id` is a declared reader.
    pub fn read(&self, node_id: &str, key: &str) -> Result<Option<&ValueEnvelope>, String> {
        if !self.readers.iter().any(|r| r.node_id == node_id) {
            return Err(format!("`{node_id}` is not a declared reader"));
        }
        Ok(self.store.get(key))
    }
}

// ---- transitive taint ------------------------------------------------------

/// Propagate taint along graph edges to a fixpoint (deterministic, order-
/// independent). `sources` seeds the taint of source nodes; each node's taint is
/// the join of every incoming edge's transferred taint (a declared `sanitizer`
/// on an edge downgrades it). Bounded iteration handles cycles safely.
pub fn propagate_taint(
    edges: &[(String, String, Option<String>)],
    sources: &BTreeMap<String, TaintSet>,
) -> BTreeMap<String, TaintSet> {
    let mut sorted = edges.to_vec();
    sorted.sort();
    let mut taint = sources.clone();
    // Fail-closed: an unseeded ROOT (an edge `src` that is never a `dst` and was
    // not declared a source) is data-from-nowhere — seed it Untrusted so an
    // empty/unknown taint can NEVER arm a downstream host action (law #6).
    let dsts: std::collections::BTreeSet<&String> = sorted.iter().map(|(_, d, _)| d).collect();
    for (src, _, _) in &sorted {
        if !taint.contains_key(src) && !dsts.contains(src) {
            taint.insert(src.clone(), TaintSet::from_labels([TaintLabel::Untrusted]));
        }
    }
    // at most |edges|+1 passes converges a monotone (join-only) lattice.
    for _ in 0..=sorted.len() {
        let mut changed = false;
        for (src, dst, sanitizer) in &sorted {
            let incoming = taint.get(src).cloned().unwrap_or_default();
            let transferred = transfer_taint(&incoming, sanitizer.as_deref());
            let current = taint.get(dst).cloned().unwrap_or_default();
            let joined = current.join(&transferred);
            if joined != current {
                taint.insert(dst.clone(), joined);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    taint
}

#[cfg(test)]
mod tests {
    use super::*;
    use zyal_core::ActionClass;

    #[test]
    fn parses_duration_strings_and_bare_ms() {
        assert_eq!(parse_duration_ms("500ms"), Some(500));
        assert_eq!(parse_duration_ms("15s"), Some(15_000));
        assert_eq!(parse_duration_ms("2m"), Some(120_000));
        assert_eq!(parse_duration_ms("1h"), Some(3_600_000));
        assert_eq!(parse_duration_ms("250"), Some(250)); // bare number = ms
        assert_eq!(parse_duration_ms("nope"), None);
        assert_eq!(parse_duration_ms(""), None);
    }

    #[test]
    fn feed_spec_honors_the_ir_duration_string() {
        // The compiled IR emits `poll_interval: "15s"`, not `poll_interval_ms`.
        let s = DataFeedSpec::from_value(
            "px",
            &serde_json::json!({ "kind": "stock_ticker", "poll_interval": "15s" }),
        );
        assert_eq!(s.poll_interval_ms, 15_000);
        // numeric form still works + wins when both present
        let s2 = DataFeedSpec::from_value(
            "px",
            &serde_json::json!({ "poll_interval_ms": 2_000, "poll_interval": "9s" }),
        );
        assert_eq!(s2.poll_interval_ms, 2_000);
    }

    fn spec(kind: &str) -> DataFeedSpec {
        DataFeedSpec::from_value(
            "market",
            &json!({ "kind": kind, "source": "example.com/quotes", "symbols": ["SPX"], "primary": "value" }),
        )
    }

    #[test]
    fn connector_authorize_call_enforces_every_limit() {
        let c = ConnectorSpec::from_value(
            "db",
            &json!({
                "kind": "api", "capabilities": ["sql.read"],
                "egress_allowlist": ["api.example.com"], "rate_limit_per_min": 5, "max_bytes": 1000
            }),
        );
        assert!(c
            .authorize_call("sql.read", "api.example.com", 500, 0)
            .is_ok());
        // subdomain matches the allowlist
        assert!(c
            .authorize_call("sql.read", "v2.api.example.com", 1, 0)
            .is_ok());
        assert_eq!(
            c.authorize_call("sql.write", "api.example.com", 1, 0)
                .unwrap_err(),
            ConnectorDenial::CapabilityDenied
        );
        assert_eq!(
            c.authorize_call("sql.read", "evil.com", 1, 0).unwrap_err(),
            ConnectorDenial::EgressDenied
        );
        assert_eq!(
            c.authorize_call("sql.read", "api.example.com", 1, 5)
                .unwrap_err(),
            ConnectorDenial::RateLimited
        );
        assert_eq!(
            c.authorize_call("sql.read", "api.example.com", 2000, 0)
                .unwrap_err(),
            ConnectorDenial::SizeExceeded
        );
    }

    #[test]
    fn poll_delay_backs_off_deterministically() {
        let s = spec("stock_ticker");
        let base = next_poll_delay_ms(&s, 0, 123);
        assert_eq!(base, s.poll_interval_ms.max(1));
        let d1 = next_poll_delay_ms(&s, 1, 123);
        let d3 = next_poll_delay_ms(&s, 3, 123);
        assert!(
            d3 >= d1 && d1 >= base,
            "backoff must be monotonic: {base} {d1} {d3}"
        );
        // deterministic for a fixed seed (replayable, no clock/rng)
        assert_eq!(next_poll_delay_ms(&s, 3, 123), d3);
        // capped
        assert!(next_poll_delay_ms(&s, 30, 999) <= MAX_BACKOFF_MS);
    }

    #[test]
    fn shared_memory_gates_writes_by_binding_and_taint() {
        let writers = vec![
            MemoryBinding {
                node_id: "feed".into(),
                allow_taint: ALL_TAINT_LABELS.to_vec(),
            },
            MemoryBinding {
                node_id: "strict".into(),
                allow_taint: vec![TaintLabel::Trusted],
            },
        ];
        let readers = vec![MemoryBinding {
            node_id: "agent".into(),
            allow_taint: vec![],
        }];
        let mut mem = SharedMemory::new(MemoryScope::Run, Retrieval::Key, writers, readers);
        // a default external-feed envelope is WebContent-tainted
        let web = envelope_from_tick(&spec("file_tail"), "r", 0, json!({"value": 1.0}), 0);
        assert!(web.taint.has(TaintLabel::WebContent));
        // "feed" permits all taint → write ok
        assert!(mem.write("feed", "k", web.clone()).is_ok());
        // "strict" permits only Trusted → a WebContent value is rejected
        assert!(mem.write("strict", "k2", web.clone()).is_err());
        // a non-writer is rejected
        assert!(mem.write("ghost", "k3", web).is_err());
        // reader access is binding-gated too
        assert!(mem.read("agent", "k").unwrap().is_some());
        assert!(mem.read("intruder", "k").is_err());
    }

    #[test]
    fn external_feed_envelope_is_web_tainted_and_hashed() {
        let s = spec("stock_ticker");
        let env = envelope_from_tick(
            &s,
            "run-1",
            3,
            json!({ "value": 42.5, "symbol": "SPX" }),
            1000,
        );
        assert_eq!(env.primary_value, Some(42.5));
        assert!(env.provenance.content_hash.starts_with("sha256:"));
        assert!(env.taint.has(TaintLabel::WebContent));
        // a web-tainted value cannot arm a host action without a sanitizer (law #6)
        assert!(!env.taint.can_arm(ActionClass::ArmHostAction));
    }

    #[test]
    fn feeds_are_untrusted_by_default_and_trust_is_explicit() {
        // a file_tail with no trust declaration is fail-closed (web_content)
        let untrusted = envelope_from_tick(&spec("file_tail"), "r", 0, json!({"value": 1.0}), 0);
        assert!(!untrusted.taint.can_arm(ActionClass::ArmHostAction));
        // only an explicit `trust: trusted` makes it clean
        let ts =
            DataFeedSpec::from_value("local", &json!({ "kind": "file_tail", "trust": "trusted" }));
        let trusted = envelope_from_tick(&ts, "r", 0, json!({"value": 1.0}), 0);
        assert!(trusted.taint.can_arm(ActionClass::ArmHostAction));
    }

    #[test]
    fn unseeded_root_is_fail_closed_untrusted() {
        // an edge from an unseeded source `x` → `y`: x is data-from-nowhere
        let edges = vec![("x".to_string(), "y".to_string(), None)];
        let taint = propagate_taint(&edges, &BTreeMap::new());
        assert!(taint["x"].has(TaintLabel::Untrusted));
        assert!(
            !taint["y"].can_arm(ActionClass::ArmHostAction),
            "an unseeded root must never arm a downstream host action"
        );
    }

    #[test]
    fn source_url_credentials_are_redacted_in_provenance() {
        let s = DataFeedSpec::from_value(
            "f",
            &json!({ "kind": "online_data", "source": "https://user:secretpass@api.example.com/feed?api_key=sk-leak" }),
        );
        let env = envelope_from_tick(&s, "r", 0, json!({"value": 1.0}), 0);
        assert!(!env.provenance.source.contains("secretpass"));
        assert!(!env.provenance.source.contains("sk-leak"));
        assert!(env.provenance.source.contains("<redacted>"));
    }

    #[test]
    fn content_hash_is_field_order_independent() {
        let s = spec("stock_ticker");
        let a = envelope_from_tick(&s, "r", 0, json!({"value": 1.0, "symbol": "X"}), 0);
        let b_val: Value = serde_json::from_str("{\"symbol\":\"X\",\"value\":1.0}").unwrap();
        let b = envelope_from_tick(&s, "r", 0, b_val, 0);
        assert_eq!(a.provenance.content_hash, b.provenance.content_hash);
    }

    #[test]
    fn feed_tick_frame_is_compact_and_carries_taint_and_prov() {
        let env = envelope_from_tick(&spec("stock_ticker"), "r", 7, json!({"value": 99.9}), 0);
        let frame = feed_tick_frame(&env, "Market feed");
        let bytes = serde_json::to_string(&frame).unwrap();
        assert!(
            bytes.len() <= 512,
            "feed_tick frame must be ≤512B, was {}",
            bytes.len()
        );
        assert_eq!(frame["feed"]["latest_value"], 99.9);
        assert!(frame["taint"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "web_content"));
        assert!(frame["prov"].as_str().unwrap().starts_with("sha256:"));
    }

    #[test]
    fn fixture_source_is_deterministic() {
        let s = spec("stock_ticker");
        let p = FixtureSourceProvider;
        assert_eq!(p.tick(&s, 5), p.tick(&s, 5));
        assert_ne!(p.tick(&s, 5), p.tick(&s, 6));
    }

    #[test]
    fn connectors_are_deny_by_default() {
        let c = ConnectorSpec::from_value(
            "db",
            &json!({ "kind": "database", "capabilities": ["sql.read"], "egress_allowlist": ["db.internal"] }),
        );
        assert!(c.allows("sql.read"));
        assert!(!c.allows("sql.write")); // not granted → denied
        assert!(c.allows_egress("db.internal") && c.allows_egress("primary.db.internal"));
        assert!(!c.allows_egress("evil.com"));
    }

    #[test]
    fn taint_propagates_transitively_and_a_sanitizer_downgrades() {
        // a → b → c → d, source a is web_content; d arms a host action
        let edges = vec![
            ("a".to_string(), "b".to_string(), None),
            ("b".to_string(), "c".to_string(), None),
            ("c".to_string(), "d".to_string(), None),
        ];
        let mut sources = BTreeMap::new();
        sources.insert(
            "a".to_string(),
            TaintSet::from_labels([TaintLabel::WebContent]),
        );
        let taint = propagate_taint(&edges, &sources);
        // taint flowed all the way to d
        assert!(taint["d"].has(TaintLabel::WebContent));
        assert!(!taint["d"].can_arm(ActionClass::ArmHostAction));

        // with a sanitizer on c→d, d may arm the action
        let edges2 = vec![
            ("a".to_string(), "b".to_string(), None),
            ("b".to_string(), "c".to_string(), None),
            (
                "c".to_string(),
                "d".to_string(),
                Some("html_strip".to_string()),
            ),
        ];
        let taint2 = propagate_taint(&edges2, &sources);
        assert!(taint2["d"].has(TaintLabel::Sanitized));
        assert!(taint2["d"].can_arm(ActionClass::ArmHostAction));
    }
}
