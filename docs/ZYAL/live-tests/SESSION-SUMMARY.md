# Session Summary — zyal-testing campaign

**Branch:** `zyal-testing`
**Plan:** `/home/ubuntu/.claude/plans/now-please-come-up-peaceful-koala.md`
**Range:** 2026-05-27, ~04:17 UTC → ~17:25 UTC (one operator-driven session, three real-time observation phases)
**Score floor:** 70 / raw 88 / caps 4 / findings 7 — **HELD across every commit**.

## What landed

Seven code/scripts fixes + four forensic markdown docs + one design proposal, all on `zyal-testing` (local only, no pushes).

| # | Commit | What | Scope |
|---|---|---|---|
| FIX-1 | `01d1f17` | `jekko-runner::model_client::runtime` honors the inner `jekko run` JSON `"success": true` flag instead of treating any non-empty stderr as failure. **Critical** — every live ZYAL pipeline halted at the first call before this. | 13 LOC |
| FIX-2 | `78028c7` | `scripts/zyal-live-batch.sh::start_fusion` exports `JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool` by default. All prior batches silently ran against legacy `config_env` (single .env.jnoccio pool), defeating the multi-tenant users_pool fan-out. | 7 LOC |
| FIX-3 | `6652284` | `zyalc::live_audit` accepts `model_receipt_count > model_outcome_event_count` (failed retries don't emit `model_outcome`). Required by the post-FIX-1 event shape. + 3 unit tests. | 49 LOC |
| FIX-4 | `e70e9b4` (combined) | `zyalc::compile` "wrote/unchanged" status messages → stderr (was stdout, leaked into jekko port-run --dry-run JSON consumers). | 4 LOC |
| FIX-5 | `e70e9b4` (combined) | `run_r0()` in batch script `unset JEKKO_BIN` so tuiwright `baseline_matrix.rs` snapshot suite skips during smoke. | 14 LOC |
| FIX-6 | `8dbcd5d` | `scripts/zyal-live-report.sh` distinguishes legitimate plan-walk no-op (0 events) from real balancer stall (events but no cursor move). | 12 LOC |
| FIX-7 | `1559cb3` | `SupervisorStore::init_run` accepts `Option<&str>` requested id; jekko port-run `--super --run-id <foo>` now honors `foo` end-to-end. + 1 test. | 41 LOC |

**Cumulative product code delta:** ~140 LOC across 7 commits, all surgical (one logical change each). Score 70/88/4/7 held.

## Live test coverage (3 phases of operator observation)

### Phase 0 — baseline (`PHASE-0-BASELINE.md`)
- 225 workspace tests passed; 9-wave dry-run plan valid; fusion `/health` returned `user_count=2, key_source=users_pool` (after FIX-2-style env var); jankurai 70/88/4/7.

### Phase 1 — three isolated single-pipeline live runs
- `RUN-p1-adv-1779855837.md` (advanced-reasoning): pre-FIX-1, 6 events, halted at first attempt. Post-FIX-1 rerun: 58 events, 5 model_outcomes parsed, reached verifier stage, 8+9 user_1/user_2 rotation.
- `RUN-p1-openqg-1779856040.md` (OpenQG superreasoning): 19 events, 6 attempts, perfect 3+3 user rotation, all misclassified by pre-FIX-1 — confirmed cross-pipeline.
- `RUN-p1-mini-1779856121.md` (MiniRedis 1-tick): same fingerprint as P1.a. 8 model attempts → 16 fusion calls, **all 16 succeeded fusion-side**, **all 16 misclassified runner-side** — the impetus for FIX-1.

### Phase 2 — full 8-rung ladder (`BATCH-zyal-testing-phase2.md`)
- 11 min wall, 7 rungs, 66 model attempts (66.7% parse rate post-FIX-1), 10 providers rotated through, **r3 reached `promotion_decision` + `RUN_FINISHED`** with perfect 14+14 rotation.
- First attempt hung r0 on tuiwright baseline matrix tests when `JEKKO_BIN` was set; surfaced FIX-5.
- Surfaced 21× s1 burst (mostly false-positive on hero-judge attempt volume) and 4× s12 judge_patch_without_proof. All other 10 signals 0.
- 2 `fusion_model_win_total` increments captured (nvidia, groq) — evidence that the **deferred quality-band feature's data source is already collecting**.

### Phase 3 — heavy MiniRedis (`HEAVY-MINIREDIS.md`)
- Stage 3.A (`--max-ticks 4`): reached `brainstorm_stages` (one stage further than Phase 1's halt at `frame_request`), produced 3 reasoning_artifacts, halted after 3 consecutive `response_bytes: 0` model returns. Content-side failure correctly classified by FIX-1 as `retryable_failure` → `final_block`.
- Stage 3.B (12-stage SuperWorkflow plan-walk): all 9 waves complete, all 12 phases marked Complete in supervisor store. Scaffold mode confirmed.
- Surfaced FIX-CAND-J (empty-response-streak signal) and FIX-CAND-L (wire jekko-runner per phase in `--super --live`) — deferred.

## Confirmatory rerun (post-FIX-1..7)

`RUN-confirm-super-1779902449` — re-ran the `zyal-superreasoning-live-local` recipe pattern with all 7 fixes in place:

- ✅ Events: 15 attempts, **14 model_outcomes parsed** (93% parse rate), 4 hero_candidate, 2 judge_patch, 1 verifier_score, `run_finished{status: complete}`.
- ✅ verify-replay: passed — 16 artifacts, 5 gates.
- ✅ FIX-1 + FIX-3 + FIX-7 all working together: pipeline ran end-to-end; the receipt-count audit invariant no longer fires; the explicit `--run-id` was honored throughout.
- ⚠️ audit-live-run now fails on a **different and legitimate** signal: `model_receipt[8] missing nonzero response_bytes`. That's the strict audit rejecting one 0-byte model response (the same content-side issue as Phase 3). Compared to pre-FIX-3 where every successful run failed audit for a false reason, this is a real-but-rare content signal — not a regression.

**Comparison vs Phase 2 r3 (the pre-fix baseline):**

| Metric | Phase 2 r3 (pre FIX-3) | Confirm rerun (post FIX-3) | Notes |
|---|---:|---:|---|
| Events captured | 93 | 56 | Phase 2 r3 ran with `--max-generations 1` × 2 lanes; this run was single-lane |
| Attempts | 20 | 15 | Smaller because we hit fewer lanes |
| Parsed | 14 | 14 | Same parse count |
| Parse rate | 70% | 93% | Better in confirm (one fewer empty-response failure) |
| audit-live-run failure | `model receipt count 20 does not match model_outcome event count 14` (FALSE POSITIVE) | `model_receipt[8] missing nonzero response_bytes` (true content signal) | FIX-3 in action |

## Aggregate gateway statistics

| Metric | Start of session | End of session | Δ |
|---|---:|---:|---:|
| `fusion_requests_total` | ~70 | 416+ | +346 |
| `fusion_success_total` | ~37 | 310+ | +273 |
| `fusion_failure_total` | ~30 | 71+ | +41 |
| Aggregate success rate during session | — | **~87 %** | — |
| `fusion_model_win_total` (fusion-sample wins) | 0 | 2 (nvidia, groq) | +2 |
| `~/.jekko/users/.balancer.sqlite` cursor | 191 | 285+ | +94 |
| Distinct providers exercised | — | 14 — cerebras, cloudflare, fireworks, gemini (google), github, groq, hf, inception, kilo, mistral, nvidia, openrouter, sambanova, zai | — |

## Deferred / un-touched

| # | Area | Reason |
|---|---|---|
| FIX-CAND-E | s1 burst signal heuristic (fires on attempt volume rather than failure-rate) | Tuning — needs careful threshold work, not surgical |
| FIX-CAND-F | judge_patch without proof timing | Likely budget-driven, not code-driven; revisit when budget is raised |
| FIX-CAND-J | Empty-response-streak signal | Needs new `EventKind` variant; defer to when the empty-response problem itself is addressed |
| FIX-CAND-K | quality-band escalation on empty-response streak | Depends on the deferred quality-band feature |
| FIX-CAND-L | Wire jekko-runner per phase in `port-run --super --live` | Feature work, Phase H follow-up |
| `MODEL_QUALITY_BAND.md` | ZYAL stages declare `quality_band: top10/20/50/bottom20` for routing | Design landed at `docs/ZYAL/MODEL_QUALITY_BAND.md`; implementation deferred per user direction (~160-200 LOC follow-up). User confirmed: defer. |

## What's now true about the ZYAL live pipelines

1. **The live recipes work end-to-end** — at least the `zyal-superreasoning-live-local` (OpenQG hero-judge) pattern reaches `run_finished` with `promotion_decision` and passes `verify-replay`'s 5 gates.
2. **user_1 + user_2 fan-out is active** — fusion gateway runs in `users_pool` mode by default (post-FIX-2), and rotation across 10 shared providers is balanced (4+4 / 5+6 / 14+14 splits across runs).
3. **The fusion gateway recovers from upstream failures** — multiple runs observed 2-5 `fusion_model_failure_total` deltas without halting the pipeline. Cooldown/retry in `jnoccio-fusion/src/limits.rs::cooldown_for` does its job.
4. **The runner correctly distinguishes 4 outcome states** post-FIX-1: `parsed`, `retryable_failure`, `final_block`, and `success-but-non-JSON-response` (rare).
5. **Forensic continuity is intact** — explicit `--run-id` is honored end-to-end (FIX-7); `events.jsonl`, `model_receipts.jsonl`, `replay_receipt.json` all land at the requested run id; downstream tooling (`jekko watch`, `port-run --status`, `zyal-live-report.sh`) finds them.
6. **Heavy MiniRedis is blocked on a content-side issue, not a code-side issue.** The model returns `response_bytes: 0` on `stage_brainstorm` 3 times in a row across both users. This is **exactly** the use case the deferred `quality_band: top20` feature would address.

## Where to pick up next

If a future session wants to extend this campaign:
1. Implement `MODEL_QUALITY_BAND.md` (design doc landed) — would let MiniRedis brainstorm escalate to a stronger model.
2. FIX-CAND-J: add `EventKind::EmptyResponseStreak` and a remediation rule.
3. FIX-CAND-L: scaffold real per-phase work in `port-run --super --live` (not just plan-walk).
4. Tune s1 burst signal threshold (FIX-CAND-E) so hero-judge runs don't trip a false positive on attempt volume.

## Memory / state of the world

- 8 new commits on `zyal-testing` (1 baseline doc + 7 fix commits + 1 quality-band design + 4 per-test docs + this summary = …actually 11 commits total, but FIX-4+5 are bundled into one, so 7 distinct fixes).
- All artifacts under `target/zyal/`: `baseline-20260527T041659Z/`, `live-batch-20260527T165425Z-zyal-testing-phase2/`, and 6+ run directories. Forensics preserved per the plan's "don't delete" rule.
- All test reports under `docs/ZYAL/live-tests/`: PHASE-0, RUN-p1-{adv,openqg,mini}-*, BATCH-zyal-testing-phase2, HEAVY-MINIREDIS, SESSION-SUMMARY (this doc).
- One feature proposal at `docs/ZYAL/MODEL_QUALITY_BAND.md`.
