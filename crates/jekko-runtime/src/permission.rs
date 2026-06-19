//! Permission ask flow.
//!
//! Ported from `packages/jekko/src/permission/index.ts` and
//! `packages/jekko/src/permission/evaluate.ts`. The TS layer combines a
//! ruleset evaluator (wildcard matcher) with an event-driven ask queue;
//! we mirror that here on top of [`crate::bus::Bus`].
//!
//! Event tags (must match the TS strings so cross-runtime subscribers
//! continue to work):
//!
//! - `permission.asked`   — published whenever an ask is awaiting reply.
//! - `permission.replied` — published when the user (or auto-replier)
//!   answers an outstanding ask.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

use crate::bus::Bus;
use crate::error::{RuntimeError, RuntimeResult};

/// Three-valued permission decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionDecision {
    /// Allow without prompting.
    Allow,
    /// Deny outright.
    Deny,
    /// Prompt the user.
    Ask,
}

/// One permission rule: a (permission, pattern) pair plus an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRule {
    /// The permission kind (e.g. `"bash"`, `"read"`, `"edit"`).
    pub permission: String,
    /// Wildcard pattern matched against the request target.
    pub pattern: String,
    /// Decision to apply when both wildcards match.
    pub action: PermissionDecision,
}

/// Permission ask payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    /// Request id.
    pub id: String,
    /// Session that issued the request.
    pub session_id: String,
    /// Permission kind being asked about.
    pub permission: String,
    /// Targets the LLM wants to use the permission against.
    pub patterns: Vec<String>,
    /// Free-form metadata (carried through to the reply event).
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// Patterns the user may auto-allow when replying `"always"`.
    #[serde(default)]
    pub always: Vec<String>,
}

/// User reply to a [`PermissionRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionReply {
    /// Allow this single call only.
    Once,
    /// Allow this call and add patterns to the approved ruleset.
    Always,
    /// Reject this call (optionally with feedback).
    Reject,
}

/// Permission service.
#[derive(Debug)]
pub struct PermissionService {
    bus: Arc<Bus>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    approved: Vec<PermissionRule>,
    pending: HashMap<String, Pending>,
}

#[derive(Debug)]
struct Pending {
    request: PermissionRequest,
    tx: oneshot::Sender<Result<PermissionReply, RuntimeError>>,
}

impl PermissionService {
    /// Construct a new service backed by `bus`.
    pub fn new(bus: Arc<Bus>) -> Self {
        Self {
            bus,
            state: Mutex::new(State::default()),
        }
    }

    /// Replace the approved ruleset.
    pub async fn set_approved(&self, ruleset: Vec<PermissionRule>) {
        let mut state = self.state.lock().await;
        state.approved = ruleset;
    }

    /// Return a snapshot of the approved ruleset.
    pub async fn approved(&self) -> Vec<PermissionRule> {
        self.state.lock().await.approved.clone()
    }

    /// Ask permission to perform `request`. If the ruleset already covers
    /// every pattern with an `allow`, the call returns immediately. Mixed
    /// outcomes fall through to a real ask: a `permission.asked` event is
    /// emitted on the bus and the call awaits a [`Self::reply`].
    ///
    /// `ruleset` is the local-session ruleset (config-derived); it stacks
    /// on top of the in-memory approved ruleset.
    pub async fn ask(
        &self,
        request: PermissionRequest,
        ruleset: Vec<PermissionRule>,
    ) -> RuntimeResult<PermissionReply> {
        // Non-interactive SCOPED auto-approve: a headless run (e.g. a bounded-queue
        // worker invoking `jekko run`) has no one to answer a prompt, so a tool that
        // needs `Ask` would block forever. An operator may pre-authorize a specific
        // set of permission kinds via env (comma-separated; `*` = all). Off by
        // default. e.g. JEKKO_PERMISSION_ALLOW=websearch,webfetch,read,grep,glob
        if let Ok(allow) = std::env::var("JEKKO_PERMISSION_ALLOW") {
            let permitted = allow
                .split(',')
                .map(str::trim)
                .any(|kind| kind == "*" || kind == request.permission.as_str());
            if permitted {
                return Ok(PermissionReply::Once);
            }
        }

        // Headless DENY: a non-interactive run (e.g. a bounded-queue worker) that opts
        // into headless mode can never answer a prompt, so an un-pre-authorized
        // permission must DENY (error the tool) rather than block the run forever. This
        // only RESTRICTS — it never grants — so a worker can run safely without hanging
        // on, e.g., a stray `bash` call. Combine with JEKKO_PERMISSION_ALLOW to permit a
        // specific read-only set while denying everything else.
        if std::env::var("JEKKO_PERMISSION_HEADLESS").as_deref() == Ok("1") {
            let target = request.patterns.first().map(String::as_str).unwrap_or("*");
            return Err(RuntimeError::PermissionDenied(format!(
                "{}:{} (headless: not pre-authorized via JEKKO_PERMISSION_ALLOW)",
                request.permission, target
            )));
        }

        // Pre-evaluate against the combined ruleset.
        let mut needs_ask = false;
        let approved = self.approved().await;
        for pattern in &request.patterns {
            let rule = evaluate(&request.permission, pattern, &[&ruleset, &approved]);
            match rule.action {
                PermissionDecision::Allow => continue,
                PermissionDecision::Deny => {
                    return Err(RuntimeError::PermissionDenied(format!(
                        "{}:{}",
                        request.permission, pattern
                    )));
                }
                PermissionDecision::Ask => needs_ask = true,
            }
        }
        if !needs_ask {
            return Ok(PermissionReply::Once);
        }

        // Emit ask + register pending.
        let (tx, rx) = oneshot::channel();
        let pending = Pending {
            request: request.clone(),
            tx,
        };
        {
            let mut state = self.state.lock().await;
            state.pending.insert(request.id.clone(), pending);
        }
        let _ = self
            .bus
            .publish(
                "permission.asked",
                serde_json::to_value(&request).map_err(RuntimeError::Json)?,
            )
            .await;

        // Wait for reply.
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(RuntimeError::PermissionRejected("dropped".into())),
        }
    }

    /// Resolve a pending request. The reply is published on the bus and
    /// the waiter is woken with `Ok(reply)`. If the reply is
    /// [`PermissionReply::Always`], the always-patterns are pushed onto
    /// the approved ruleset.
    pub async fn reply(&self, request_id: &str, reply: PermissionReply) -> RuntimeResult<()> {
        let pending = {
            let mut state = self.state.lock().await;
            match state.pending.remove(request_id) {
                Some(p) => p,
                None => {
                    return Err(RuntimeError::not_found("permission", request_id));
                }
            }
        };

        // Publish reply on the bus.
        let payload = serde_json::json!({
            "sessionID": pending.request.session_id,
            "requestID": pending.request.id,
            "reply": reply,
        });
        let _ = self.bus.publish("permission.replied", payload).await;

        // Update approved ruleset on `always`.
        if matches!(reply, PermissionReply::Always) {
            let mut state = self.state.lock().await;
            for pattern in &pending.request.always {
                state.approved.push(PermissionRule {
                    permission: pending.request.permission.clone(),
                    pattern: pattern.clone(),
                    action: PermissionDecision::Allow,
                });
            }
        }

        // Signal waiter.
        match reply {
            PermissionReply::Reject => {
                let _ = pending.tx.send(Err(RuntimeError::PermissionRejected(
                    pending.request.id.clone(),
                )));
            }
            other => {
                let _ = pending.tx.send(Ok(other));
            }
        }

        Ok(())
    }

    /// List currently pending requests.
    pub async fn list_pending(&self) -> Vec<PermissionRequest> {
        let state = self.state.lock().await;
        state.pending.values().map(|p| p.request.clone()).collect()
    }
}

/// Evaluate a ruleset against `(permission, pattern)`.
///
/// Mirrors `packages/jekko/src/permission/evaluate.ts`: the **last** matching
/// rule wins, with both permission and pattern matched as wildcards. Falls
/// back to `Ask` when nothing matches.
pub fn evaluate(permission: &str, pattern: &str, rulesets: &[&[PermissionRule]]) -> PermissionRule {
    let mut last: Option<PermissionRule> = None;
    for ruleset in rulesets {
        for rule in *ruleset {
            if wildcard_match(permission, &rule.permission)
                && wildcard_match(pattern, &rule.pattern)
            {
                last = Some(rule.clone());
            }
        }
    }
    match last {
        Some(rule) => rule,
        None => PermissionRule {
            permission: permission.to_string(),
            pattern: DEFAULT_PERMISSION_PATTERN.to_string(),
            action: PermissionDecision::Ask,
        },
    }
}

/// Default pattern used when no rule matches and we synthesise an `Ask` rule.
const DEFAULT_PERMISSION_PATTERN: &str = "*";

/// Generate a fresh permission request id.
pub fn new_request_id() -> String {
    format!("perm_{}", Uuid::new_v4().simple())
}

/// Match `value` against a wildcard `pattern`. Supports `*` and `?`.
///
/// Mirrors the semantics of `packages/jekko/src/util/wildcard.ts`.
pub fn wildcard_match(value: &str, pattern: &str) -> bool {
    let v = value.as_bytes();
    let p = pattern.as_bytes();
    let (mut i, mut j, mut star_i, mut star_j) = (0_usize, 0_usize, None, 0_usize);
    while i < v.len() {
        if j < p.len() && (p[j] == b'?' || p[j] == v[i]) {
            i += 1;
            j += 1;
        } else if j < p.len() && p[j] == b'*' {
            star_i = Some(i);
            star_j = j;
            j += 1;
        } else if let Some(si) = star_i {
            j = star_j + 1;
            star_i = Some(si + 1);
            i = si + 1;
        } else {
            return false;
        }
    }
    while j < p.len() && p[j] == b'*' {
        j += 1;
    }
    j == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matches() {
        assert!(wildcard_match("read", "*"));
        assert!(wildcard_match("read", "read"));
        assert!(wildcard_match("read", "r??d"));
        assert!(wildcard_match("read_file", "read*"));
        assert!(!wildcard_match("write", "read*"));
    }

    #[test]
    fn evaluate_last_match_wins() {
        let global = vec![PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionDecision::Ask,
        }];
        let local = vec![PermissionRule {
            permission: "read".into(),
            pattern: "*".into(),
            action: PermissionDecision::Allow,
        }];
        let rule = evaluate("read", "/foo", &[&global, &local]);
        assert_eq!(rule.action, PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn ask_resolves_via_reply() {
        let bus = Arc::new(Bus::new());
        let svc = Arc::new(PermissionService::new(bus.clone()));

        let mut sub = bus.subscribe("permission.asked").await;
        let req = PermissionRequest {
            id: new_request_id(),
            session_id: "session_x".into(),
            permission: "bash".into(),
            patterns: vec!["ls".into()],
            metadata: serde_json::json!({}),
            always: vec!["ls".into()],
        };
        let req_id = req.id.clone();
        let svc_clone = svc.clone();
        let h = tokio::spawn(async move { svc_clone.ask(req, vec![]).await });

        let env = sub.recv().await.unwrap();
        assert_eq!(env.kind, "permission.asked");

        svc.reply(&req_id, PermissionReply::Always).await.unwrap();
        let reply = h.await.unwrap().unwrap();
        assert_eq!(reply, PermissionReply::Always);

        let approved = svc.approved().await;
        assert_eq!(approved.len(), 1);
        assert_eq!(approved[0].pattern, "ls");
    }

    #[tokio::test]
    async fn ask_resolves_immediately_when_allowed() {
        let bus = Arc::new(Bus::new());
        let svc = PermissionService::new(bus);
        let ruleset = vec![PermissionRule {
            permission: "read".into(),
            pattern: "*".into(),
            action: PermissionDecision::Allow,
        }];
        let req = PermissionRequest {
            id: new_request_id(),
            session_id: "session_x".into(),
            permission: "read".into(),
            patterns: vec!["/etc".into()],
            metadata: serde_json::json!({}),
            always: vec![],
        };
        let reply = svc.ask(req, ruleset).await.unwrap();
        assert_eq!(reply, PermissionReply::Once);
    }

    #[tokio::test]
    async fn deny_returns_error() {
        let bus = Arc::new(Bus::new());
        let svc = PermissionService::new(bus);
        let ruleset = vec![PermissionRule {
            permission: "read".into(),
            pattern: "*".into(),
            action: PermissionDecision::Deny,
        }];
        let req = PermissionRequest {
            id: new_request_id(),
            session_id: "session_x".into(),
            permission: "read".into(),
            patterns: vec!["/etc/passwd".into()],
            metadata: serde_json::json!({}),
            always: vec![],
        };
        let err = svc.ask(req, ruleset).await.unwrap_err();
        assert!(matches!(err, RuntimeError::PermissionDenied(_)));
    }
}
