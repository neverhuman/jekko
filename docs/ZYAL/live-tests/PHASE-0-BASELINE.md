# Phase 0 — Baseline & Wiring Check

**Timestamp:** `2026-05-27T04:16:59Z`
**Branch:** `zyal-testing`
**Plan reference:** `/home/ubuntu/.claude/plans/now-please-come-up-peaceful-koala.md`

## 1. Build status

| Target | Result | Path | Size |
|--------|--------|------|------|
| `jekko` (release) | exit 0 | `target/release/jekko` | 15.3 MiB |
| `jekko-runner` (release) | exit 0 | `target/release/jekko-runner` | (workspace member) |
| `jnoccio-fusion` (release) | exit 0 (171 crates, 54.15s) | `jnoccio-fusion/target/release/jnoccio-fusion` | 14.3 MiB |
| `jnoccio-fusion` (debug) | already present | `jnoccio-fusion/target/debug/jnoccio-fusion` | 130.4 MiB |

Builds use `unset RUSTC_WRAPPER` per memory note (sccache port collision).

## 2. Workspace test smoke (`cargo test --workspace --locked --no-fail-fast`)

- **passed:** 225
- **failed:** 0
- **ignored:** 17 (expected — gated behind `#[ignore]` + env-var guards like `AGENT_SEARCH_LIVE=1`, `JEKKO_TUI_LIVE_PROD=1`)

Full log: `target/zyal/baseline-20260527T041659Z/cargo-test.log` (rtk tee log retained at `~/.local/share/rtk/tee/1779855551_cargo_test.log`).

**Verdict:** GREEN. No regressions vs. HEAD.

## 3. Dry-run wave plan (`jekko port-run --super agent/zyal/ambitious-superworkflow.zyal --dry-run`)

- **manifest_id:** `ambitious-superworkflow-template`
- **wave_count:** 9 (≥4 required ✓)
- First wave: `[source_of_truth]` ✓
- Last wave: `[final_signoff]` ✓

```
wave 1: ['source_of_truth']
wave 2: ['architecture_blueprint', 'repo_graph_bootstrap']
wave 3: ['contracts_and_slices']
wave 4: ['parallel_subsystems', 'parity_lab']
wave 5: ['integration_fusion']
wave 6: ['parity_gap_closure']
wave 7: ['hardening_security', 'performance_closure']
wave 8: ['docs_release_ops']
wave 9: ['final_signoff']
```

Captured at `target/zyal/baseline-20260527T041659Z/dry-run.{json,clean.json}`.

### 🟡 Observation (FIX-CAND-A): `jekko port-run --dry-run` leaks compile chatter on stdout

The dry-run prints `zyalc: wrote /tmp/.tmpCmm5ha\n` on **stdout** before the JSON. Plain `python3 -m json.tool` cannot consume the output. Workaround: `tail -n +2`. Structured CLI output should be clean stdout / chatter on stderr.

- **Likely site:** `crates/zyalc/src/main.rs` or compile-on-demand path inside `crates/jekko-cli/src/cmd/port_run.rs` — print is going through `println!` instead of `eprintln!` / `tracing::info!`.
- **Severity:** low (workaround exists); but it breaks JSON consumers downstream.
- **Status:** logged for Phase 4. Not blocking Phase 1.

## 4. jnoccio-fusion startup + `/health`

Initial naïve start: `./target/debug/jnoccio-fusion --config config/server.json --env-file .env.jnoccio` came up in `config_env` (legacy single-pool) mode:

```json
{ "upstream_key_source": "config_env", "user_count": 0, "per_user_slot_counts": {} }
```

Searching the source: `jnoccio-fusion/src/config.rs:338` reads env var `JNOCCIO_UPSTREAM_KEY_SOURCE`, falls back to `server.json::upstream_key_source`, then defaults to `ConfigEnv`. The recipes export `JEKKO_KEY_SOURCE_POLICY=users-only` for `jekko-runner` — that env var is in a **different namespace** and is **not** propagated to the fusion gateway start.

### 🔴 Observation (FIX-CAND-B, FIRST CANDIDATE FOR FIX-1): live-batch.sh starts fusion without `JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool`

- **File:** `scripts/zyal-live-batch.sh::start_fusion` (around line 154–170)
- **Symptom:** Fusion gateway runs in `ConfigEnv` mode in all prior batches — the multi-tenant user-pool path (`UsersPool`) the entire ZYAL multi-user campaign relies on is **never exercised**. user_1/user_2 keys may still be used by jekko-runner's internal path, but the fusion gateway side advertises only the legacy single-pool.
- **Confirmation:** zero scripts in the repo grep-match `JNOCCIO_UPSTREAM_KEY_SOURCE` or `users_pool`.
- **Severity:** **high** — defeats the architectural premise of `~/.jekko/users/{user_1,user_2}/`.
- **Minimal fix:** export `JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool` in `start_fusion()` (or read `JEKKO_KEY_SOURCE_POLICY` from the caller and translate). Phase 4 FIX-1.
- **Workaround used for this campaign:** I started fusion manually with the env var set.

### After the env-var fix — current `/health`

```json
{
  "available_models": 131,
  "eligible_slot_count": 102,
  "keyed_models": 130,
  "missing_keys": ["alibaba/alibaba-qwen3-coder-plus"],
  "upstream_key_source": "users_pool",
  "user_count": 2,
  "per_user_slot_counts": {"user_1": 59, "user_2": 43},
  "per_provider_slot_counts": {
    "cerebras": 2,  "cloudflare": 12, "fireworks": 4,  "github": 10,
    "google": 2,    "groq": 8,        "huggingface": 6,"kilo": 8,
    "mistral": 6,   "nvidia": 12,     "openrouter": 24,"sambanova": 6,
    "vercel": 1,    "zai": 1
  }
}
```

Comparison ConfigEnv vs UsersPool: 79 → 131 models (1.66×), 59 → 102 eligible slots. Captured at `target/zyal/baseline-20260527T041659Z/fusion-{health.json,metrics.prom}`.

## 5. User-slot inventory (`~/.jekko/users/*/llm.env`)

| Slot   | Total keys | Shared with the other slot | Unique to this slot |
|--------|-----------:|----------------------------|----------------------|
| user_1 | 16         | (shared 10, below)         | AI_GATEWAY, HF_TOKEN, INCEPTION, KILO, ZAI |
| user_2 | 11         | (shared 10, below)         | — (subset of user_1) |

**Shared providers (10) — the rotation-evidence set:** CEREBRAS, CLOUDFLARE, FIREWORKS, GEMINI (GOOGLE), GITHUB, GROQ, MISTRAL, NVIDIA, OPENROUTER, SAMBANOVA.

Both slots additionally contain `JNOCCIO_DEVELOPER_KEY` (unlocked developer key — confirmed present, value not printed).

## 6. Balancer state snapshot (`~/.jekko/users/.balancer.sqlite`)

**Schema:**
```sql
CREATE TABLE round_robin_cursor (
  provider TEXT NOT NULL,
  model    TEXT NOT NULL,
  cursor   INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (provider, model)
);
```

**Rows at baseline:** 1 (essentially pristine — single carry-over from prior probe). Saved at `target/zyal/baseline-20260527T041659Z/balancer-pristine.sql`.

### 🟡 Observation (FIX-CAND-C): cursor key is `(provider, model)` — no user dimension

The cursor partition is `(provider, model)` only. The Plan-mode design agent flagged this as a potential bottleneck for per-user-set rotation. If two pipelines simultaneously bind to the same `(provider, model)`, they'll alternate users via a single shared cursor — that's actually the **desired** behavior for round-robin across user_1/user_2 (and is fine). The risk is if a job needs *sticky* per-user assignment (e.g., the same user across all steps of one run) — there's no per-run/per-job dimension. Phase 3 will measure cursor advancement to confirm rotation works.

- **Decision for now:** observe, don't pre-fix. Only act if Phase 3 shows the `balancer_no_rotation` signal.

## 7. Jankurai audit baseline

```
score=70 raw=88 caps=4 findings=7
```

**Audit json/md:** `.jankurai/repo-score.{json,md}` (also updated to history at `target/jankurai/score-history.jsonl`).

**Caps applied (4):**
1. `fallback-soup-in-product-code` (cap 70) ← binding floor
2. `missing-rendered-ux-qa-lane` (cap 84)
3. `agent-tool-supply-chain-gap` (cap 78)
4. `ci-bad-behavior` (cap 72)

**Hard findings inventory (7) — must not regress:**

| # | Sev | Rule | Path | Reason |
|---|-----|------|------|--------|
| 1 | medium | HLT-001 (shape) | `.` | Largest authored file `reasoning_io.rs` 496 LOC + fallback soup marker |
| 2 | high   | HLT-034 (security) | `.gitlab-ci.yml:1` | `ci.concurrency.missing` |
| 3 | high   | HLT-034 (security) | `.gitlab-ci.yml:1` | `ci.permissions.missing` |
| 4 | high   | HLT-034 (security) | `.gitlab-ci.yml:1` | `ci.timeout.missing` |
| 5 | high   | HLT-024 (security) | `agent/zyal/ambitious-superworkflow.zyal:15` | non-open sentinel |
| 6 | high   | HLT-013 (ux-qa)    | no web surface | UX QA lane is intentionally TUI-backed |
| 7 | high   | HLT-001 (vibe)     | `crates/jekko-runner/src/classifier.rs:141` | `let cap_marker = cap_id.unwrap_or_default()` |

**Score floor for this campaign:** `final_score >= 70`, `raw >= 88`, `caps_applied == [those 4]`, `findings <= 7`. Any FIX-N that triggers a NEW finding or cap is reverted before commit.

## 8. Phase 0 pass criteria — final summary

| Criterion | Required | Observed | Pass? |
|-----------|----------|----------|-------|
| cargo test workspace green | exit 0, failed=0 | 225/0/17 | ✅ |
| Dry-run JSON valid with ≥4 waves | yes | 9 waves | ✅ |
| First wave contains `source_of_truth` | yes | yes | ✅ |
| Last wave contains `final_signoff` | yes | yes | ✅ |
| fusion `/metrics` returns Prom text | yes | yes | ✅ |
| Balancer snapshot saved | yes | 1-row dump saved | ✅ |
| Jankurai `final_score >= 70`, `raw >= 88` | yes | 70/88 | ✅ |
| Hard findings ≤ 7 | yes | 7 | ✅ |

**Overall: Phase 0 PASS.** Proceeding to Phase 1.

## 9. Carry-forward fix candidates (queue for Phase 4)

1. **FIX-1 (highest priority):** `scripts/zyal-live-batch.sh::start_fusion` does not export `JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool`. All prior live runs went through ConfigEnv-mode fusion. **Minimum diff:** one `export` line near line 154. Then run a rerun to compare batch `report.json`.
2. **FIX-CAND-A:** `jekko port-run --dry-run` writes `zyalc: wrote /tmp/...` to **stdout** before the JSON. Should be `eprintln!` or `tracing::info!`. Likely in `crates/zyalc/src/main.rs` or `crates/jekko-cli/src/cmd/port_run/parse.rs`.
3. **FIX-CAND-C (observe only for now):** Balancer cursor key `(provider, model)` — no user / run-id dimension. Phase 3 will confirm whether this matters.

## 10. Next: Phase 1

Proceed with three isolated single-pipeline live runs. Fusion is up at `127.0.0.1:4317` in `users_pool` mode with 2 users / 102 eligible slots.
