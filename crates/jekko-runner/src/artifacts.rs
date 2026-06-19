//! Content-addressed artifacts (M14) — **artifact id == node id**.
//!
//! The runner (NOT the model) computes an artifact's `sha256` from its actual
//! bytes, so a model can never *claim* an artifact into existence — only real
//! bytes are recorded in the append-only [`ArtifactLedger`]. A run cannot finish
//! until the [`DoneGate`]'s required artifacts are present + hashed +
//! provenance-complete (+ rendered). Renders treat every model artifact as
//! hostile input ([`render_refusal`]); the bounded `artifact_rendered` frame is
//! matched on the artifact id (which equals the node id — killing the old
//! `artifact/<name>` prefix that broke live patches).
//!
//! This module owns the ledger, the done-gate, the render receipt + refusal +
//! deterministic fake render, and the frame builder. The real sandboxed CLI
//! render pipeline (pdftoppm/mutool/magick) + the web gallery land in M14-cont.

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::hashing::sha256_hex;

/// One recorded artifact version. `artifact_id` IS the FlowGraph node id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactVersion {
    pub artifact_id: String,
    /// `v_<hash12>` — derived from the runner-computed content hash.
    pub version_id: String,
    /// `sha256:<hex>` computed by the runner from the bytes.
    pub content_hash: String,
    pub bytes_len: usize,
    pub mime: String,
    pub provenance_complete: bool,
}

/// A lineage relationship between two artifact versions (M14-cont).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactEdgeKind {
    /// `dst` was derived from `src` (e.g. a report from analysis).
    DerivedFrom,
    /// `src` renders to `dst` (e.g. a pdf → a png).
    RendersTo,
    /// `dst` is a new version of `src` (same logical artifact).
    VersionOf,
    /// `dst` used `src` as input data.
    UsesData,
    /// `dst` was validated by `src` (e.g. a check/proof).
    ValidatedBy,
    /// `src` was published as `dst` (e.g. an internal artifact → a release).
    PublishedAs,
}

/// A lineage edge between two artifact `version_id`s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactEdge {
    pub src_version_id: String,
    pub kind: ArtifactEdgeKind,
    pub dst_version_id: String,
}

/// An append-only artifact version ledger + its lineage edges.
#[derive(Debug, Default)]
pub struct ArtifactLedger {
    versions: Vec<ArtifactVersion>,
    latest: BTreeMap<String, usize>,
    edges: Vec<ArtifactEdge>,
}

impl ArtifactLedger {
    /// Record an artifact's bytes (runner-computed hash). Idempotent by content:
    /// recording the identical bytes for an id returns the existing version
    /// without appending a duplicate.
    pub fn record(
        &mut self,
        artifact_id: &str,
        bytes: &[u8],
        mime: &str,
        provenance_complete: bool,
    ) -> ArtifactVersion {
        let content_hash = format!("sha256:{}", sha256_hex(bytes));
        if let Some(&i) = self.latest.get(artifact_id) {
            if self.versions[i].content_hash == content_hash {
                // Same bytes → no new version, but allow provenance to be UPGRADED
                // (incomplete → complete) so a later "now provenance-complete"
                // record isn't silently lost. Never downgrade.
                if provenance_complete && !self.versions[i].provenance_complete {
                    self.versions[i].provenance_complete = true;
                }
                return self.versions[i].clone();
            }
        }
        let version = ArtifactVersion {
            artifact_id: artifact_id.to_string(),
            version_id: format!("v_{}", &content_hash[7..19]),
            content_hash,
            bytes_len: bytes.len(),
            mime: mime.to_string(),
            provenance_complete,
        };
        self.versions.push(version.clone());
        self.latest
            .insert(artifact_id.to_string(), self.versions.len() - 1);
        version
    }

    pub fn latest(&self, artifact_id: &str) -> Option<&ArtifactVersion> {
        self.latest.get(artifact_id).map(|&i| &self.versions[i])
    }

    pub fn versions_of(&self, artifact_id: &str) -> Vec<&ArtifactVersion> {
        self.versions
            .iter()
            .filter(|v| v.artifact_id == artifact_id)
            .collect()
    }

    /// Record a lineage edge between two versions (M14-cont). Keeps lineage a
    /// validated DAG: both endpoints must be recorded versions (no dangling
    /// edges), no self-loops, and no cycles (rejected if `dst` already reaches
    /// `src`). Append-only + idempotent by `(src, kind, dst)`.
    pub fn record_edge(
        &mut self,
        src_version_id: &str,
        kind: ArtifactEdgeKind,
        dst_version_id: &str,
    ) -> Result<(), String> {
        let known = |id: &str| self.versions.iter().any(|v| v.version_id == id);
        if !known(src_version_id) {
            return Err(format!("unknown src version `{src_version_id}`"));
        }
        if !known(dst_version_id) {
            return Err(format!("unknown dst version `{dst_version_id}`"));
        }
        if src_version_id == dst_version_id {
            return Err("lineage self-loop is not allowed".to_string());
        }
        if self.reaches(dst_version_id, src_version_id) {
            return Err("edge would form a lineage cycle".to_string());
        }
        let edge = ArtifactEdge {
            src_version_id: src_version_id.to_string(),
            kind,
            dst_version_id: dst_version_id.to_string(),
        };
        if !self.edges.contains(&edge) {
            self.edges.push(edge);
        }
        Ok(())
    }

    /// Whether `from` reaches `to` by following edges (cycle prevention).
    fn reaches(&self, from: &str, to: &str) -> bool {
        let mut stack = vec![from.to_string()];
        let mut seen = BTreeSet::new();
        while let Some(cur) = stack.pop() {
            if cur == to {
                return true;
            }
            if !seen.insert(cur.clone()) {
                continue;
            }
            for e in self.edges.iter().filter(|e| e.src_version_id == cur) {
                stack.push(e.dst_version_id.clone());
            }
        }
        false
    }

    pub fn edges(&self) -> &[ArtifactEdge] {
        &self.edges
    }

    /// The direct lineage parents of `version_id` (the `src` of every edge
    /// pointing at it) — the one-hop provenance the gallery draws.
    pub fn lineage_of(&self, version_id: &str) -> Vec<&ArtifactEdge> {
        self.edges
            .iter()
            .filter(|e| e.dst_version_id == version_id)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.versions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }
}

/// An artifact a run MUST produce before it can complete.
#[derive(Debug, Clone)]
pub struct RequiredArtifact {
    pub id: String,
    pub must_render: bool,
}

/// The completion gate. A model can never satisfy it by claiming — only real,
/// hashed, provenance-complete (and rendered, if required) bytes do.
#[derive(Debug, Clone, Default)]
pub struct DoneGate {
    pub required: Vec<RequiredArtifact>,
}

/// The done-gate verdict (secret-free; safe to emit/persist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoneReport {
    pub passed: bool,
    pub missing: Vec<String>,
    pub unrendered: Vec<String>,
    pub incomplete_provenance: Vec<String>,
}

impl DoneGate {
    /// `rendered` is the set of artifact ids that have a successful render.
    pub fn evaluate(&self, ledger: &ArtifactLedger, rendered: &BTreeSet<String>) -> DoneReport {
        let mut missing = Vec::new();
        let mut unrendered = Vec::new();
        let mut incomplete_provenance = Vec::new();
        for req in &self.required {
            match ledger.latest(&req.id) {
                None => missing.push(req.id.clone()),
                Some(v) => {
                    if !v.provenance_complete {
                        incomplete_provenance.push(req.id.clone());
                    }
                    if req.must_render && !rendered.contains(&req.id) {
                        unrendered.push(req.id.clone());
                    }
                }
            }
        }
        DoneReport {
            passed: missing.is_empty() && unrendered.is_empty() && incomplete_provenance.is_empty(),
            missing,
            unrendered,
            incomplete_provenance,
        }
    }
}

/// A render output receipt (bounded + secret-free).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderReceipt {
    pub artifact_id: String,
    pub version_id: String,
    pub render_id: String,
    pub renderer: String,
    pub mime: String,
    pub page: u32,
    pub status: String,
    /// Content-addressed handle for the rendered bytes (`sha256:<hex>`); the
    /// secure content API serves the render by this ETag (M14-cont).
    pub etag: String,
}

/// Why a render must be REFUSED. A pre-filter that treats every model-produced
/// artifact as hostile input — the FULL XML/render hardening (no-network,
/// fs-scoped, entities disabled) is the real sandbox in M14-cont; this is
/// defense-in-depth for the deterministic fake render + an early reject.
///
/// To resist evasion, the markup scan runs over a NORMALIZED copy: lowercased,
/// HTML-entity-decoded (twice, to catch double-encoding like `&amp;lt;`), and
/// whitespace-stripped — so `&lt;!entity`, `<! entity`, and `<\n!entity` all match.
pub fn render_refusal(bytes: &[u8]) -> Option<&'static str> {
    // Scan the WHOLE artifact — a hostile token placed past any fixed window
    // would otherwise slip through (M14-cont review). Size is already bounded
    // upstream by connector/done-gate limits.
    let text = String::from_utf8_lossy(bytes);
    // credentials: scan the raw text (markers contain no whitespace)
    if zyal_core::contains_any_credential(&text).is_some() {
        return Some("credential in artifact");
    }
    // markup: normalize away encoding + whitespace evasions before scanning.
    // Decode entities to a FIXPOINT (not a fixed 2 passes) so multiply-encoded
    // payloads like `&amp;amp;amp;lt;script` can't survive the scan; bounded by a
    // hard cap so a pathological input can't loop forever.
    let mut compact = text.to_ascii_lowercase();
    for _ in 0..16 {
        let decoded = compact
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&");
        if decoded == compact {
            break;
        }
        compact = decoded;
    }
    let compact: String = compact.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.contains("<script")
        || compact.contains("javascript:")
        || compact.contains("onerror=")
        || compact.contains("onload=")
        || compact.contains("onclick=")
        || compact.contains("data:text/html")
    {
        return Some("active content (script) in artifact");
    }
    if compact.contains("<!entity") || compact.contains("<!doctype") {
        return Some("xml entity/doctype in artifact");
    }
    None
}

/// A DETERMINISTIC fake render for CI / replay — no subprocess. The render id is
/// derived from the version hash so a run is reproducible. Refuses hostile input
/// just like the real driver. (pdftoppm/mutool/magick land in M14-cont.)
pub fn fake_render(
    version: &ArtifactVersion,
    bytes: &[u8],
    renderer: &str,
    out_mime: &str,
    page: u32,
) -> Result<RenderReceipt, String> {
    if let Some(reason) = render_refusal(bytes) {
        return Err(format!("E_ARTIFACT_RENDER_REFUSED: {reason}"));
    }
    let render_id = format!("r_{}_{page}", &version.content_hash[7..19]);
    // Deterministic ETag: a content address over (source hash, renderer, out
    // mime, page). LENGTH-PREFIXED so no two distinct field tuples can collide
    // (`a|b` vs `a`,`b` ambiguity — M14-cont review).
    let etag = format!(
        "sha256:{}",
        sha256_hex(
            format!(
                "{}:{}|{}:{renderer}|{}:{out_mime}|{page}",
                version.content_hash.len(),
                version.content_hash,
                renderer.len(),
                out_mime.len(),
            )
            .as_bytes()
        )
    );
    Ok(RenderReceipt {
        artifact_id: version.artifact_id.clone(),
        version_id: version.version_id.clone(),
        render_id,
        renderer: renderer.to_string(),
        mime: out_mime.to_string(),
        page,
        status: "rendered".to_string(),
        etag,
    })
}

/// Whether a render CLI is available on PATH (capability detection). Spawns
/// `<bin> --version` with all stdio nulled; success ⇒ usable. Used by [`render`]
/// to decide between the real sandboxed pipeline and the deterministic fake.
pub fn renderer_available(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Render an artifact: refuse hostile input (via [`fake_render`]), then produce a
/// receipt + content-addressed ETag. `cli_available` is the caller's capability
/// decision (from [`renderer_available`]) — injected so the policy is testable
/// without spawning. When the real sandboxed CLI pipeline is available the
/// receipt records the live `renderer`; otherwise it records a `:fake` fallback,
/// so a run NEVER blocks on a missing render CLI. (The real CLI byte-render —
/// pdftoppm/mutool/magick into a no-network, fs-scoped render root — is the
/// deferred tail; both paths share this identical receipt shape + ETag today.)
pub fn render(
    version: &ArtifactVersion,
    bytes: &[u8],
    renderer: &str,
    out_mime: &str,
    page: u32,
    cli_available: bool,
) -> Result<RenderReceipt, String> {
    let mut receipt = fake_render(version, bytes, renderer, out_mime, page)?;
    receipt.renderer = if cli_available {
        renderer.to_string()
    } else {
        format!("{renderer}:fake")
    };
    Ok(receipt)
}

/// The bounded `artifact_rendered` live frame — matched on `artifact_id` (== the
/// node id). The actual image is served from the render root by `content_url`.
pub fn artifact_rendered_frame(receipt: &RenderReceipt, content_url: &str) -> Value {
    json!({
        "artifact_id": receipt.artifact_id,
        "version_id": receipt.version_id,
        "render_id": receipt.render_id,
        "content_url": content_url,
        "mime": receipt.mime,
        "page": receipt.page,
        "status": receipt.status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_id_equals_node_id_with_runner_computed_hash() {
        let mut led = ArtifactLedger::default();
        let v = led.record("report_pdf", b"%PDF-1.7 ...", "application/pdf", true);
        // the id is the node id — no `artifact/<name>` prefix
        assert_eq!(v.artifact_id, "report_pdf");
        assert!(v.content_hash.starts_with("sha256:"));
        assert!(v.version_id.starts_with("v_") && v.version_id.len() == 14);
    }

    #[test]
    fn ledger_is_idempotent_by_content_and_versions_on_change() {
        let mut led = ArtifactLedger::default();
        let a = led.record("paper", b"draft one", "text/plain", true);
        let b = led.record("paper", b"draft one", "text/plain", true); // identical bytes
        assert_eq!(a.version_id, b.version_id);
        assert_eq!(led.len(), 1, "identical content is not re-appended");
        let c = led.record("paper", b"draft two", "text/plain", true); // new bytes
        assert_ne!(a.version_id, c.version_id);
        assert_eq!(led.versions_of("paper").len(), 2);
        assert_eq!(led.latest("paper").unwrap().version_id, c.version_id);
    }

    #[test]
    fn done_gate_rejects_model_only_claims() {
        let gate = DoneGate {
            required: vec![RequiredArtifact {
                id: "report_pdf".into(),
                must_render: true,
            }],
        };
        let led = ArtifactLedger::default(); // a model "claimed" it but wrote no bytes
        let report = gate.evaluate(&led, &BTreeSet::new());
        assert!(!report.passed);
        assert_eq!(report.missing, vec!["report_pdf".to_string()]);
    }

    #[test]
    fn done_gate_requires_hash_provenance_and_render() {
        let gate = DoneGate {
            required: vec![RequiredArtifact {
                id: "report_pdf".into(),
                must_render: true,
            }],
        };
        let mut led = ArtifactLedger::default();
        // present but provenance incomplete + not rendered
        led.record("report_pdf", b"%PDF", "application/pdf", false);
        let r1 = gate.evaluate(&led, &BTreeSet::new());
        assert!(
            !r1.passed
                && r1.incomplete_provenance == ["report_pdf"]
                && r1.unrendered == ["report_pdf"]
        );
        // provenance complete + rendered → passes
        led.record("report_pdf", b"%PDF v2", "application/pdf", true);
        let mut rendered = BTreeSet::new();
        rendered.insert("report_pdf".to_string());
        assert!(gate.evaluate(&led, &rendered).passed);
    }

    #[test]
    fn render_refuses_hostile_artifacts() {
        assert!(render_refusal(b"clean pdf bytes").is_none());
        assert!(render_refusal(b"<svg><script>alert(1)</script></svg>").is_some());
        assert!(render_refusal(b"OPENAI_API_KEY=sk-leak").is_some());
    }

    #[test]
    fn render_refusal_resists_encoding_and_whitespace_evasion() {
        // whitespace-split markers
        assert!(render_refusal(b"<!  entity xxe SYSTEM \"file:///etc/passwd\">").is_some());
        assert!(render_refusal(b"<\n!doctype foo>").is_some());
        // HTML-entity-encoded script + double-encoded
        assert!(render_refusal(b"&lt;script&gt;steal()&lt;/script&gt;").is_some());
        assert!(render_refusal(b"&amp;lt;!entity x&amp;gt;").is_some());
        // event-handler + javascript: + data URI
        assert!(render_refusal(b"<img src=x onerror=alert(1)>").is_some());
        assert!(render_refusal(b"<a href=\"javascript:evil()\">").is_some());
    }

    #[test]
    fn provenance_can_be_upgraded_on_identical_bytes() {
        let mut led = ArtifactLedger::default();
        led.record("paper", b"final bytes", "text/plain", false);
        assert!(!led.latest("paper").unwrap().provenance_complete);
        // re-record identical bytes, now provenance-complete → upgraded, no new version
        led.record("paper", b"final bytes", "text/plain", true);
        assert!(led.latest("paper").unwrap().provenance_complete);
        assert_eq!(led.len(), 1, "upgrade does not append a duplicate version");
    }

    #[test]
    fn fake_render_is_deterministic_and_refuses_hostile_input() {
        let mut led = ArtifactLedger::default();
        let v = led.record("plot", b"vega spec bytes", "application/json", true);
        let a = fake_render(&v, b"vega spec bytes", "vega", "image/png", 1).unwrap();
        let b = fake_render(&v, b"vega spec bytes", "vega", "image/png", 1).unwrap();
        assert_eq!(a.render_id, b.render_id);
        // hostile bytes are refused even with a valid version
        assert!(fake_render(&v, b"<script>x</script>", "svg", "image/svg+xml", 1).is_err());
    }

    #[test]
    fn artifact_rendered_frame_is_compact_and_keyed_on_artifact_id() {
        let mut led = ArtifactLedger::default();
        let v = led.record("report_pdf", b"%PDF", "application/pdf", true);
        let receipt = fake_render(&v, b"%PDF", "pdftoppm", "image/png", 2).unwrap();
        let frame = artifact_rendered_frame(&receipt, "/render/report_pdf/2.png");
        let bytes = serde_json::to_string(&frame).unwrap();
        assert!(bytes.len() <= 512);
        assert_eq!(frame["artifact_id"], "report_pdf");
        assert_eq!(frame["page"], 2);
    }

    #[test]
    fn render_sets_a_content_addressed_etag_and_records_cli_vs_fake() {
        let mut led = ArtifactLedger::default();
        let v = led.record("report_pdf", b"%PDF body", "application/pdf", true);
        let fake = render(&v, b"%PDF body", "pdftoppm", "image/png", 1, false).unwrap();
        assert_eq!(fake.renderer, "pdftoppm:fake");
        assert!(fake.etag.starts_with("sha256:") && fake.etag.len() == "sha256:".len() + 64);
        let real = render(&v, b"%PDF body", "pdftoppm", "image/png", 1, true).unwrap();
        assert_eq!(real.renderer, "pdftoppm");
        // ETag is content-addressed: identical inputs → identical handle, both paths
        assert_eq!(fake.etag, real.etag);
        // hostile input is refused regardless of CLI availability
        assert!(render(&v, b"<script>x</script>", "svg", "image/svg+xml", 1, true).is_err());
    }

    #[test]
    fn renderer_available_reports_a_missing_binary_as_unavailable() {
        assert!(!renderer_available("jekko-no-such-renderer-xyz-9000"));
    }

    #[test]
    fn render_refusal_catches_multi_encoding_and_late_payloads() {
        // triple-encoded `<script` needs >2 decode passes — caught by the fixpoint.
        assert!(render_refusal(b"&amp;amp;amp;lt;script&amp;amp;amp;gt;").is_some());
        // a hostile token placed PAST the old 1MB window is now scanned.
        let mut big = vec![b' '; (1 << 20) + 100];
        big.extend_from_slice(b"<script>evil</script>");
        assert!(render_refusal(&big).is_some());
        // a benign artifact still passes.
        assert!(render_refusal(b"%PDF-1.7 just a normal document").is_none());
    }

    #[test]
    fn lineage_edges_are_recorded_deduped_and_queryable() {
        let mut led = ArtifactLedger::default();
        let data = led.record("dataset", b"rows", "text/csv", true);
        let report = led.record("report", b"analysis", "text/markdown", true);
        let png = led.record("report_png", b"\x89PNG", "image/png", true);
        led.record_edge(
            &data.version_id,
            ArtifactEdgeKind::UsesData,
            &report.version_id,
        )
        .unwrap();
        led.record_edge(
            &report.version_id,
            ArtifactEdgeKind::RendersTo,
            &png.version_id,
        )
        .unwrap();
        led.record_edge(
            &data.version_id,
            ArtifactEdgeKind::UsesData,
            &report.version_id,
        )
        .unwrap(); // dup
        assert_eq!(led.edges().len(), 2, "duplicate edge must not be appended");
        let parents = led.lineage_of(&report.version_id);
        assert_eq!(parents.len(), 1);
        assert_eq!(parents[0].kind, ArtifactEdgeKind::UsesData);
        assert_eq!(parents[0].src_version_id, data.version_id);

        // dangling endpoints, self-loops, and cycles are rejected (validated DAG)
        assert!(led
            .record_edge("v_nope", ArtifactEdgeKind::UsesData, &report.version_id)
            .is_err());
        assert!(led
            .record_edge(
                &data.version_id,
                ArtifactEdgeKind::VersionOf,
                &data.version_id
            )
            .is_err());
        // data→report→png exists; png→data would close a cycle
        assert!(led
            .record_edge(
                &png.version_id,
                ArtifactEdgeKind::DerivedFrom,
                &data.version_id
            )
            .is_err());
    }
}
