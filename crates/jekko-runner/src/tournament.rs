//! Tournament / reasoning kernel (M10) — the canonical construct `hero_judge`
//! and legacy `experiments` lower into.
//!
//! The distinctive, Rust-enforced piece is **blind cross-provider judging**:
//! candidates are anonymized by content hash before judging (the hash→id map
//! NEVER leaves the runner), and a judge that shares a provider/family with any
//! generator is flagged ineligible — so a model can never recognise + favour its
//! own output. Generations are loop-back metadata, never a visual axis.
//!
//! This module owns the spec types + the blind-judging enforcement (the trust
//! boundary). The live execution (real model calls per lane, verifier/refute
//! panels, self-repair) reuses `hero_judge_eval::reduce_generation` and lands in
//! M10-cont.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::hashing::sha256_hex;

/// How many generations the tournament runs. `forever` requires a budget + an
/// explicit confirmation token (enforced by the compiler/runner, not here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "policy")]
pub enum GenerationPolicy {
    Once,
    Bounded { max: u32 },
    Forever { confirm: String },
    UntilConverged { max: u32 },
    UntilDry { max: u32 },
    UntilBudget,
}

/// How generations RENDER — never a visual axis. The default is a single body +
/// one curved loop-back edge (constant node count regardless of generation count).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationRender {
    LoopBack,
    Unroll,
    Auto,
}

/// The canonical tournament construct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TournamentSpec {
    pub id: String,
    pub objective: String,
    pub generations: GenerationPolicy,
    pub render: GenerationRender,
    pub lanes: u32,
    pub judges: u32,
    pub verifiers: u32,
    pub refute_lanes: u32,
}

/// A reasoning-lane output to be judged.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub candidate_id: String,
    pub provider: String,
    pub family: String,
    pub content: String,
}

/// What a judge sees: ONLY the content hash + a rank slot. No id, provider, or
/// family — so judging cannot be biased by origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlindBallot {
    pub candidate_hash: String,
    pub rank: u32,
}

/// The blind-judging setup for one judge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlindResult {
    /// Anonymized candidates, ordered by hash (position reveals nothing).
    pub ballots: Vec<BlindBallot>,
    /// `false` when this judge shares a provider/family with a generator — it
    /// must be excluded (or promotion capped) to keep judging independent.
    pub eligible_judge: bool,
    /// `true` when blindness/independence could not be guaranteed (emits a
    /// `JudgeBlindnessDegraded` event upstream).
    pub degraded: bool,
}

/// Anonymize candidates for a judge and check independence. Returns the blind
/// ballots PLUS the hash→candidate_id map kept SEPARATELY — the map is
/// runner-only and must never be serialized into an event/receipt/prompt.
pub fn enforce_blind(
    candidates: &[Candidate],
    judge_provider: &str,
    judge_family: &str,
) -> (BlindResult, BTreeMap<String, String>) {
    let mut map = BTreeMap::new();
    let mut ballots = Vec::with_capacity(candidates.len());
    for c in candidates {
        let hash = format!("cand:{}", &sha256_hex(c.content.as_bytes())[..16]);
        map.insert(hash.clone(), c.candidate_id.clone());
        ballots.push(BlindBallot {
            candidate_hash: hash,
            rank: 0,
        });
    }
    // Order by hash so the ballot position leaks nothing about which lane / which
    // generation produced a candidate.
    ballots.sort_by(|a, b| a.candidate_hash.cmp(&b.candidate_hash));
    ballots.dedup_by(|a, b| a.candidate_hash == b.candidate_hash);

    let shares_origin = candidates
        .iter()
        .any(|c| c.provider == judge_provider || c.family == judge_family);

    (
        BlindResult {
            ballots,
            eligible_judge: !shares_origin,
            degraded: shares_origin,
        },
        map,
    )
}

/// De-anonymize ranked ballots back to candidate ids (runner-only) after judging.
/// Unknown hashes are dropped. Returns `(candidate_id, rank)` pairs.
pub fn deanonymize(ranked: &[BlindBallot], map: &BTreeMap<String, String>) -> Vec<(String, u32)> {
    ranked
        .iter()
        .filter_map(|b| map.get(&b.candidate_hash).map(|id| (id.clone(), b.rank)))
        .collect()
}

/// The winning candidate id from ranked blind ballots (rank 1 = best), or `None`.
pub fn winner(ranked: &[BlindBallot], map: &BTreeMap<String, String>) -> Option<String> {
    ranked
        .iter()
        .filter(|b| b.rank > 0)
        .min_by_key(|b| b.rank)
        .and_then(|b| map.get(&b.candidate_hash).cloned())
}

// ---- live tournament execution (M10-cont) ----------------------------------

/// The content hash a candidate is judged under (content-only, so it is stable
/// across judges — the same answer hashes the same everywhere). Public so the
/// run-loop dispatch layer can construct a judge's `BlindBallot`s against the
/// same canonical id the kernel de-anonymizes by.
pub fn candidate_hash(content: &str) -> String {
    format!("cand:{}", &sha256_hex(content.as_bytes())[..16])
}

/// One judge's identity + the ranking it produced over the blind ballots it saw
/// (rank 1 = best). In production a judge model emits the ranking from the blind
/// ballots; here it is the judging result so the orchestration is deterministic.
#[derive(Debug, Clone)]
pub struct JudgeInput {
    pub provider: String,
    pub family: String,
    pub ranking: Vec<BlindBallot>,
}

/// The verifier panel's verdict on the proposed winner. `passed` is a
/// DETERMINISTIC check result (a tool/shell check, a quote audit, a proof) —
/// NEVER a model's self-claim of success. Green is earned, not asserted.
#[derive(Debug, Clone)]
pub struct VerifierVerdict {
    pub passed: bool,
    pub detail: String,
}

/// The outcome of one tournament generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentOutcome {
    pub winner_id: Option<String>,
    pub promoted: bool,
    pub judges_total: usize,
    pub judges_eligible: usize,
    /// Blindness/independence was degraded for ≥1 judge (emits `JudgeBlindnessDegraded`).
    pub degraded: bool,
    pub verified: bool,
    pub refuted: bool,
    pub reason: String,
}

/// Run ONE tournament generation end-to-end over already-produced candidates:
/// blind-judge across the panel (excluding judges that share a generator's
/// provider/family), aggregate ELIGIBLE judges to a winner, verify it
/// deterministically, let refute lanes attack it, then gate promotion.
///
/// Promotion requires ALL of: a winner, an eligible-judge majority, a passing
/// (deterministic) verifier, and surviving refutation. A model can never promote
/// its own output (blind + independence) nor self-certify success (verifier is a
/// real check). `verify`/`refute` are injected so the trust logic is testable
/// without live model calls.
pub fn run_tournament(
    candidates: &[Candidate],
    judges: &[JudgeInput],
    verify: impl Fn(&Candidate) -> VerifierVerdict,
    refute: impl Fn(&Candidate) -> bool,
) -> TournamentOutcome {
    // Stable content-hash → candidate-id map (the de-anonymization key, runner-only).
    let global_map: BTreeMap<String, String> = candidates
        .iter()
        .map(|c| (candidate_hash(&c.content), c.candidate_id.clone()))
        .collect();

    // Aggregate ranks across ELIGIBLE judges only (lower summed rank = better).
    let mut rank_sum: BTreeMap<String, u32> = BTreeMap::new();
    let mut eligible = 0usize;
    let mut degraded = false;
    for j in judges {
        let (res, _map) = enforce_blind(candidates, &j.provider, &j.family);
        if res.degraded {
            degraded = true;
        }
        if !res.eligible_judge {
            continue; // a judge sharing a generator's origin cannot vote
        }
        eligible += 1;
        for b in &j.ranking {
            if b.rank > 0 && global_map.contains_key(&b.candidate_hash) {
                *rank_sum.entry(b.candidate_hash.clone()).or_insert(0) += b.rank;
            }
        }
    }

    // Winner = lowest summed rank (ties broken by hash for determinism).
    let winner_id = rank_sum
        .iter()
        .min_by(|a, b| a.1.cmp(b.1).then_with(|| a.0.cmp(b.0)))
        .and_then(|(hash, _)| global_map.get(hash).cloned());

    let winner_cand = winner_id
        .as_ref()
        .and_then(|id| candidates.iter().find(|c| &c.candidate_id == id));

    let (verified, vdetail) = match winner_cand {
        Some(c) => {
            let v = verify(c);
            (v.passed, v.detail)
        }
        None => (false, "no eligible-judge winner".to_string()),
    };
    let refuted = winner_cand.map(&refute).unwrap_or(false);

    // Promotion gate.
    let majority = eligible * 2 > judges.len();
    let promoted = winner_id.is_some() && eligible > 0 && majority && verified && !refuted;
    let reason = if promoted {
        format!(
            "promoted ({eligible}/{} eligible judges, verified: {vdetail})",
            judges.len()
        )
    } else if winner_id.is_none() {
        "no winner — no eligible judge ranked a candidate".to_string()
    } else if !majority {
        format!(
            "blocked: only {eligible}/{} judges eligible (no majority)",
            judges.len()
        )
    } else if !verified {
        format!("blocked: verifier failed ({vdetail})")
    } else if refuted {
        "blocked: winner was refuted".to_string()
    } else {
        "blocked".to_string()
    };

    TournamentOutcome {
        winner_id,
        promoted,
        judges_total: judges.len(),
        judges_eligible: eligible,
        degraded,
        verified,
        refuted,
        reason,
    }
}

// ---- self-repair (M10-cont) ------------------------------------------------

/// The outcome of a [`fix_until_green`] loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfRepairOutcome {
    /// `true` only when a deterministic check AND the completeness critic passed.
    pub green: bool,
    pub attempts: u32,
    /// The accepted fix (content), when green.
    pub fixed: Option<String>,
    /// Fixes that failed — recorded so the same broken fix is never retried.
    pub negative_memory: Vec<String>,
}

/// Repair until GREEN or attempts run out. `green` is NEVER a model self-claim:
/// `check` is a deterministic verifier (a tool/shell/proof check) and `complete`
/// is the completeness critic — BOTH must pass. `propose` receives the negative
/// memory (prior failed fixes) so it can avoid repeating them; an identical
/// re-proposal is skipped (no progress) and counts toward the attempt budget.
pub fn fix_until_green(
    mut propose: impl FnMut(&[String]) -> String,
    check: impl Fn(&str) -> bool,
    complete: impl Fn(&str) -> bool,
    max_attempts: u32,
) -> SelfRepairOutcome {
    let mut negative_memory: Vec<String> = Vec::new();
    let mut attempts = 0;
    while attempts < max_attempts {
        attempts += 1;
        let candidate = propose(&negative_memory);
        if negative_memory.contains(&candidate) {
            continue; // a known-bad fix — no progress, don't re-check
        }
        if check(&candidate) && complete(&candidate) {
            return SelfRepairOutcome {
                green: true,
                attempts,
                fixed: Some(candidate),
                negative_memory,
            };
        }
        negative_memory.push(candidate);
    }
    SelfRepairOutcome {
        green: false,
        attempts,
        fixed: None,
        negative_memory,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cands() -> Vec<Candidate> {
        vec![
            Candidate {
                candidate_id: "c0".into(),
                provider: "openai".into(),
                family: "gpt".into(),
                content: "theory A".into(),
            },
            Candidate {
                candidate_id: "c1".into(),
                provider: "anthropic".into(),
                family: "claude".into(),
                content: "theory B".into(),
            },
            Candidate {
                candidate_id: "c2".into(),
                provider: "google".into(),
                family: "gemini".into(),
                content: "theory C".into(),
            },
        ]
    }

    #[test]
    fn ballots_are_anonymized_and_leak_nothing() {
        let (res, map) = enforce_blind(&cands(), "mistral", "mixtral");
        assert_eq!(res.ballots.len(), 3);
        assert!(res.eligible_judge && !res.degraded);
        // the serialized ballots carry ONLY the hash + rank — no id/provider/family
        let json = serde_json::to_string(&res.ballots).unwrap();
        for leak in [
            "c0",
            "c1",
            "c2",
            "openai",
            "anthropic",
            "google",
            "gpt",
            "claude",
            "gemini",
            "theory",
        ] {
            assert!(!json.contains(leak), "ballot leaked `{leak}`: {json}");
        }
        // ordered by hash (deterministic, origin-free)
        let mut sorted = res.ballots.clone();
        sorted.sort_by(|a, b| a.candidate_hash.cmp(&b.candidate_hash));
        assert_eq!(res.ballots, sorted);
        // the map covers every candidate (runner-only)
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn a_judge_sharing_a_generator_family_is_ineligible() {
        // judge is also an openai/gpt model — it could recognise its own output
        let (res, _) = enforce_blind(&cands(), "openai", "gpt");
        assert!(!res.eligible_judge);
        assert!(res.degraded, "blindness is degraded when origins overlap");
    }

    #[test]
    fn deanonymize_and_winner_map_back_to_ids() {
        let (res, map) = enforce_blind(&cands(), "mistral", "mixtral");
        // rank the blind ballots (1 = best)
        let mut ranked = res.ballots.clone();
        for (i, b) in ranked.iter_mut().enumerate() {
            b.rank = (i + 1) as u32;
        }
        let back = deanonymize(&ranked, &map);
        assert_eq!(back.len(), 3);
        // the rank-1 ballot's id is the winner
        let win = winner(&ranked, &map).unwrap();
        let rank1_hash = &ranked.iter().find(|b| b.rank == 1).unwrap().candidate_hash;
        assert_eq!(&win, map.get(rank1_hash).unwrap());
    }

    #[test]
    fn generation_policy_and_render_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&GenerationRender::LoopBack).unwrap(),
            "\"loop_back\""
        );
        let p = GenerationPolicy::Bounded { max: 8 };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"policy\":\"bounded\"") && json.contains("\"max\":8"));
    }

    fn ballot(content: &str, rank: u32) -> BlindBallot {
        BlindBallot {
            candidate_hash: candidate_hash(content),
            rank,
        }
    }

    fn pass(detail: &str) -> impl Fn(&Candidate) -> VerifierVerdict + '_ {
        move |_c| VerifierVerdict {
            passed: true,
            detail: detail.to_string(),
        }
    }

    #[test]
    fn run_tournament_promotes_a_verified_unrefuted_winner() {
        let cands = cands();
        // two ELIGIBLE judges (neither shares a generator's origin) both rank c1 best
        let judges = vec![
            JudgeInput {
                provider: "mistral".into(),
                family: "mixtral".into(),
                ranking: vec![
                    ballot("theory B", 1),
                    ballot("theory A", 2),
                    ballot("theory C", 3),
                ],
            },
            JudgeInput {
                provider: "meta".into(),
                family: "llama".into(),
                ranking: vec![
                    ballot("theory B", 1),
                    ballot("theory C", 2),
                    ballot("theory A", 3),
                ],
            },
        ];
        let out = run_tournament(&cands, &judges, pass("checks pass"), |_c| false);
        assert_eq!(out.winner_id.as_deref(), Some("c1"));
        assert_eq!(out.judges_eligible, 2);
        assert!(out.promoted && out.verified && !out.refuted && !out.degraded);
    }

    #[test]
    fn an_unverified_or_refuted_winner_is_not_promoted() {
        let cands = cands();
        let judges = vec![JudgeInput {
            provider: "mistral".into(),
            family: "mixtral".into(),
            ranking: vec![ballot("theory A", 1)],
        }];
        // verifier fails → not promoted (green is earned by a real check)
        let out = run_tournament(
            &cands,
            &judges,
            |_c| VerifierVerdict {
                passed: false,
                detail: "shell check failed".into(),
            },
            |_c| false,
        );
        assert_eq!(out.winner_id.as_deref(), Some("c0"));
        assert!(!out.promoted && !out.verified);
        // verified but refuted → not promoted
        let out2 = run_tournament(&cands, &judges, pass("ok"), |_c| true);
        assert!(out2.verified && out2.refuted && !out2.promoted);
    }

    #[test]
    fn an_ineligible_judge_cannot_promote_its_own_family() {
        // the only judge shares openai/gpt with c0's generator → ineligible → no
        // eligible judge → no winner → nothing promoted.
        let cands = cands();
        let judges = vec![JudgeInput {
            provider: "openai".into(),
            family: "gpt".into(),
            ranking: vec![ballot("theory A", 1)],
        }];
        let out = run_tournament(&cands, &judges, pass("ok"), |_c| false);
        assert_eq!(out.judges_eligible, 0);
        assert!(out.degraded);
        assert!(out.winner_id.is_none() && !out.promoted);
    }

    #[test]
    fn fix_until_green_stops_at_first_deterministic_green() {
        let mut n = 0;
        let out = fix_until_green(
            |_neg| {
                n += 1;
                format!("fix-{n}")
            },
            |c| c == "fix-3", // deterministic green check (not a self-claim)
            |_c| true,        // completeness critic ok
            10,
        );
        assert!(out.green);
        assert_eq!(out.fixed.as_deref(), Some("fix-3"));
        assert_eq!(out.attempts, 3);
        assert_eq!(out.negative_memory, vec!["fix-1", "fix-2"]);
    }

    #[test]
    fn fix_until_green_needs_both_check_and_completeness() {
        // the deterministic check passes but the completeness critic fails → never green
        let out = fix_until_green(|_n| "x".into(), |_c| true, |_c| false, 3);
        assert!(!out.green && out.fixed.is_none());
    }

    #[test]
    fn fix_until_green_negative_memory_skips_repeats() {
        // a proposer that keeps returning the same broken fix records it ONCE,
        // then skips the repeats (no progress) until the budget runs out.
        let out = fix_until_green(|_neg| "same".into(), |_c| false, |_c| true, 4);
        assert!(!out.green);
        assert_eq!(out.negative_memory, vec!["same"]);
        assert_eq!(out.attempts, 4);
    }
}
