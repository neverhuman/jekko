# Model Quality Band — jnoccio-fusion routing extension

**Status:** **landed** (2026-05-27 GOD-logging session).
**Implementation:** `jnoccio-fusion/src/quality_band.rs` + filter pass in `jnoccio-fusion/src/routing.rs::select_without_replacement`. Five module-level tests + three integration tests under `mod tests` in routing.rs.
**Origin:** zyal-testing session, 2026-05-27. User request: "ZYAL should be able to request a stronger model for a task. We don't want to always pick the best model, but for some elements in ZYAL being able to require a top 10% or top 20% model could be a very powerful concept if it is leveraging our local win rate data."

## How to use

Pass `quality_band` as a field of the OpenAI request's `extra` map. Recognised values: `any` (default), `top10`, `top20`, `top50`, `bottom20`. Unknown strings fall back to `any` silently.

```jsonc
{
  "model": "jnoccio/jnoccio-router",
  "messages": [...],
  "extra": { "quality_band": "top20" }
}
```

ZYAL stage authors declare the band on a per-stage `model_policy` in their `.zyal` manifest. The runner forwards it through the OpenAI-shape request when invoking fusion, and fusion's `RequestProfile::from_request` lifts the value into the routing plan.

## Behavior

- **`Any`** (default): no filter — existing behavior, no path change.
- **`Top10` / `Top20` / `Top50`**: only models in the top N% of observed win-rate are considered. Unranked models (those with fewer than `DEFAULT_MIN_CALLS_FOR_RANKING = 20` calls) are **admitted** alongside ranked top-tier models so exploration keeps refreshing the evidence base.
- **`Bottom20`**: only models in the bottom 20% of observed win-rate. Unranked models are **rejected** (we don't blindly send to fresh models that may not work). Useful for non-critical work where the explicit goal is to use weaker models.
- **Soft-fallback**: if a band filter produces an empty candidate set, fusion logs `quality_band yielded zero candidates; falling back to full eligible set` (target `jnoccio_fusion::quality_band`, level `warn`) and proceeds with the unfiltered eligible pool. The band is a hint, not a hard gate — refusing the request would be worse than serving a weaker model.

## Data source

`model_metrics.win_count / call_count` from the persistent state DB. Already collected by the fusion-sample mechanism at `routing.fusion_sample_rate: 0.1` (server.json) — sampled calls fan out to a backup model in parallel and the winner increments `win_count`.

`RoutingModelInput` (the in-memory routing snapshot) carries `win_count` and `call_count` directly so the filter is per-request O(N log N) on the eligible-set size.

## Where this matters operationally

The heavy MiniRedis port-run halts at `stage_brainstorm` because the routed model returns `response_bytes: 0` three times in a row across both `user_1` and `user_2`. Declaring `quality_band: top20` on the brainstorm stage's `model_policy` should escalate that stage to a higher-win-rate model class and unblock the pipeline. The Phase H verification in the GOD-logging session exercises exactly this path.

## File layout

| File | LOC | Role |
|---|---:|---|
| `jnoccio-fusion/src/quality_band.rs` | 243 | `QualityBand` enum + `compute_percentiles` + `passes_band` + 5 unit tests |
| `jnoccio-fusion/src/routing.rs` | 950 | `RequestProfile.quality_band` field, parse from `extra`, filter pass in `select_without_replacement`, soft-fallback log + `win_count`/`call_count` on `RoutingModelInput`. 3 integration tests. |
| `jnoccio-fusion/src/fusion.rs` | 3500+ | (single-site) wires `win_count`/`call_count` into `RoutingModelInput` from `model_metrics`. |
| `jnoccio-fusion/src/lib.rs` | 17 | registers the `quality_band` module |

## Why this is feasible with the existing setup

The infrastructure is already there:

| Existing piece | Where | What it gives us |
|---|---|---|
| **Persistent win-rate data** | `jnoccio-fusion/src/state/state_core.rs:44,96` schema + `state_recording.rs:117,137,145,157` | `model_metrics` table tracks `(call_count, success_count, failure_count, win_count, last_latency_ms, …)` per `(model_id, provider)`. |
| **Win-rate computation** | `fusion.rs:2241-2242` | `win_rate = ratio(win_count, call_count)` already computed for the `/v1/jnoccio/metrics` snapshot. |
| **Per-model win counter** | `fusion_model_win_total{model,provider}` Prometheus metric (`OBSERVABILITY.md` line 191) | Externally observable. |
| **Competitive fusion-sample evidence** | `server.json::routing.fusion_sample_rate: 0.1` + `fusion.rs:865 complete_fusion_sample` | 10% of qualifying calls are duplicated to an alternate model; the winner increments `win_count`. This is *exactly* the random-fraction win-data source the user described. |
| **Backup/fallback rank** | `fusion.rs:464 backup_rank` | A model's eligible-set position is already tracked per request. |
| **Roulette weights** | `routing.rs:281 roulette_weight` | Multi-factor scoring (quality × health × complexity × capacity × load × latency × context × failure × **learned_rate** × overruns) — `learned_rate_factor` already consults observed evidence. |

What is *missing* is one explicit knob: **"select only from the top N% (or bottom N%) of models by observed win-rate."** Today, win-rate influences the weight but never hard-filters the candidate set.

## Proposed API surface

ZYAL passes a band in `ChatCompletionRequest.extra` (no breaking schema change — `extra: Map<String, Value>` already exists at `openai.rs:5`):

```jsonc
{
  "model": "jnoccio/jnoccio-router",
  "messages": [...],
  "extra": {
    "quality_band": "top20",  // top10 | top20 | top50 | any | bottom20
    "min_calls_for_ranking": 20  // optional, default 20 — cold-start guard
  }
}
```

Inside fusion, `RequestProfile::from_request` reads `extra["quality_band"]`, normalizes to a `QualityBand` enum, and threads it into `RoutePlan` alongside `complexity_tier`.

`select_without_replacement` (`routing.rs:334`) gains one extra filter step *after* `is_eligible` but *before* `roulette_weight`:

```rust
pool.retain(|model| passes_quality_band(model, profile.quality_band, &all_eligible_metrics));
if pool.is_empty() && profile.quality_band != QualityBand::Any {
    // Soft-fallback: log + drop the band constraint rather than fail the request.
    pool = full_eligible_set;
    emit_metric_or_event("quality_band_unmet");
}
```

`passes_quality_band` reads each model's `(win_count, call_count)`, computes win_rate, ranks all eligible models, and admits only those in the requested percentile slice. Models with `call_count < min_calls_for_ranking` are treated as "unranked" and either always-admitted (top bands, exploration) or always-rejected (bottom bands).

## Wiring path

1. **`config.rs` / `routing.rs`** — add `QualityBand` enum (`Top10 | Top20 | Top50 | Bottom20 | Any`) and `quality_band: QualityBand` field on `RequestProfile` + `RoutePlan`.
2. **`routing.rs::RequestProfile::from_request`** — parse `request.extra["quality_band"]` into the enum; default `Any`.
3. **`routing.rs::plan_route`** — pass `quality_band` from profile into route_plan and into `select_without_replacement`.
4. **`routing.rs::select_without_replacement`** — add `passes_quality_band` filter step.
5. **`routing.rs`** — new helper `compute_quality_percentiles(metrics: &[ModelMetric]) -> HashMap<String, f64>` that returns each model's percentile rank by win-rate.
6. **`fusion.rs::route_meta`** — surface `quality_band` in receipt metadata so events.jsonl shows which band was used.
7. **`OBSERVABILITY.md`** — document the new `fusion_quality_band_usage_total{band}` counter (one new Prom counter).
8. **`crates/jekko-runner/src/superreasoning/`** — adopt: stage definitions that want a stronger model add `quality_band: top10` to their model-policy entry. The Stage 2 reasoning (`brainstorm`, `critique`) might want `top20`. The non-critical Stage 8 docs-release work could use `bottom20` to free up better models for the hot path.

## Safety / risk

| Risk | Mitigation |
|---|---|
| **Cold-start** — new models have 0 wins, 0 calls | `min_calls_for_ranking` threshold; unranked models are either always-admitted (top bands, exploration) or always-rejected (bottom bands). |
| **Band-empty candidate set** | Soft-fallback to full eligible set + emit a "quality_band_unmet" event; never fail the request. |
| **Contention** when many lanes ask for `top10` | `select_without_replacement` already prefers distinct providers and respects per-hour usage caps — the existing throttles still apply *within* the band-filtered pool. |
| **Win-rate drift over weeks/months** | Already addressed by the rolling-window accounting in `state_recording.rs`. |
| **Backwards compat** | New field is optional in `extra`; default behavior unchanged. Zero new failure modes for current callers. |

## Estimated change footprint

- `jnoccio-fusion/src/routing.rs`: +60–80 LOC (enum, parser, filter, helper, plumbing).
- `jnoccio-fusion/src/fusion.rs`: +10 LOC (surface band in meta).
- `jnoccio-fusion/src/state.rs` (or `state_util.rs`): no change (data already collected).
- `jnoccio-fusion/Cargo.toml`: no new deps.
- New tests in `routing.rs`: ~80 LOC (percentile correctness, cold-start, empty-band fallback, plumb-through).
- `docs/ZYAL/OBSERVABILITY.md`: +metric row.
- `docs/ZYAL/SPEC.md`: a stanza explaining how ZYAL stages declare `quality_band`.

Net: ~160–200 LOC across product code + tests + docs. No new crates, no new dependencies, no schema migration. **All cleanly under the 500-LOC-per-file ceiling.**

## What this enables in ZYAL

- The 12-stage canonical kernel can declare per-stage band requirements:
  - `source_of_truth`: `top20` — bad evidence collection poisons the whole run.
  - `architecture_blueprint`, `contracts_and_slices`: `top10` — high-leverage decisions.
  - `parallel_subsystems`: `top20` for hot subsystems, `top50` for routine ones.
  - `parity_lab`: `top20` — false parity claims are catastrophic.
  - `hardening_security`: `top10` — security claims require the strongest model.
  - `docs_release_ops`: `bottom20` — gives weaker models meaningful work.
  - `final_signoff`: `top10` — last guard.
- Aggregate effect: critical paths get stronger models without burning the entire budget on top-tier; routine work gets distributed across the bottom tier, increasing total throughput and exercising weaker models so their win-rate evidence keeps refreshing.

## Open questions for implementation

1. Should `top10` band be **strict** (only the 10% best) or **floor** (at least top-10% by win-rate; weaker allowed if no top-10% is eligible)? Spec proposed: strict, with soft-fallback.
2. Should the band restrict primary *only*, or also restrict backups? Spec proposed: primary + backups, but `fusion_sample` continues to draw from the full pool (so win-rate evidence keeps accumulating across all tiers).
3. Should bands be persisted in the `metrics` table or recomputed per request? Spec proposed: recompute per request — eligible set is small (~100 models), cost is O(N log N).
4. Per-tier exploration: should `top10` band still occasionally explore lower-tier models? Spec proposed: yes, at `routing.exploration_floor.max(0.03)` rate, same as existing roulette logic — exploration is the only way to refresh win-rate evidence.

## Decision required

This is a real feature, not a bug fix — it lands net-new code and a new public knob. I do **not** plan to implement it inside the current zyal-testing live-test campaign without explicit confirmation. See the AskUserQuestion that accompanies this doc.
