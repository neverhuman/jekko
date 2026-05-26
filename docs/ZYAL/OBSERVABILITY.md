# ZYAL Observability

Two operator surfaces ship for live observation of a ZYAL run:

- `jekko watch <run_id>` — notify-based live tail of a single run's
  event stream with built-in remediation rules.
- `jnoccio-fusion` `/metrics` — Prometheus 0.0.4 scrape endpoint over
  the gateway-wide and per-model counters/gauges.

## `jekko watch <run_id>`

Tails `target/zyal/runs/<run_id>/events.jsonl` via the `notify` crate.
On every file-change event (debounced 150ms) the watcher re-folds the
event log into a `WatcherSnapshot`, applies the four remediation
rules, and emits a per-tick summary.

### Flags

| Flag | Default | Meaning |
|---|---|---|
| `<RUN_ID>` (positional) | — | Run id; resolves `target/zyal/runs/<run_id>/events.jsonl` under the repo root. |
| `--repo-root <PATH>` | cwd | Override the repo root used to locate the events file. |
| `--format <plain\|json\|tui>` | `plain` | Output format. See below. |
| `--once` | off | Read the existing file once and exit; no follow loop. |
| `--no-follow` | off | Equivalent to `--once` after the initial drain. |
| `--stall-threshold <SECS>` | `300` | Seconds without a progress event before `StallDetected` fires. Heartbeats do not count as progress. |
| `--error-rate-threshold <FLOAT>` | `0.5` | Provider error rate (0.0..=1.0) that triggers `ProviderErrorBurst` once at least 20 attempts are observed. |
| `--tui-once-snapshot` | off | (`--format tui` only) Render one frame to a `ratatui::backend::TestBackend`, dump to stdout, exit 0. CI helper. |

### Formats

- `plain` (default): newline-delimited human-readable summary per
  tick. One line per snapshot:
  `snapshot: lanes=<finished>/<started> workers_pass=N workers_fail=N
  gaps_open=N model_attempts=N model_failures=N spend_usd=...`, plus
  one indented line per remediation action with the rule name +
  summary + a one-line detail payload.
- `json`: pretty-printed `{snapshot, actions}` JSON object per tick.
  Suitable for piping into `jq` or a log shipper.
- `tui`: Ratatui dashboard (Phase G2). Interactive by default with
  keyboard refresh; pair with `--tui-once-snapshot` for CI assertions
  on the layout.

### Remediation rules

All four rules are evaluated by `jankurai_runner::watcher::remediation`
on every tick. Triggered rules surface as a `RemediationTriggered`
event-style action with `{rule, summary, detail}`. The watcher does
not perform side effects on its own — it surfaces; the caller acts.

| Rule | Fires when |
|---|---|
| `StallDetected` | No progress-event of any kind has been observed for `>= stall_threshold` seconds and the run is not `finished`. Heartbeats are explicitly excluded so a quiet stream still trips the rule. |
| `ProviderErrorBurst` | `failures / attempts > error_rate_threshold` over at least 20 model attempts. Detail payload includes `errors_by_provider` so the operator sees which upstream is misbehaving. |
| `ParityGapsGrowing` | Current `parity_gaps_open` is strictly greater than the value observed three ticks ago. Approximates the spec's "growing for 3 ticks" without requiring time-windowed retention. |
| `JankuraiRegression` | Current `jankurai_hard_findings` is strictly greater than the previously observed value. Anchored on `last_jankurai_score` audit events. |

### Recipe

```bash
# Plain tail in one shell.
rtk jekko watch ambitious-superworkflow-template

# Machine-readable feed.
rtk jekko watch ambitious-superworkflow-template --format json | jq .

# Lower thresholds for a noisy smoke run.
rtk jekko watch ambitious-superworkflow-template \
  --stall-threshold 60 \
  --error-rate-threshold 0.25
```

## `jnoccio-fusion` `/metrics` (Prometheus)

A `text/plain; version=0.0.4` scrape endpoint at the canonical
`/metrics` path. Mirrors the JSON dashboard data at
`/v1/jnoccio/metrics` in the line-protocol format Prometheus + Grafana
expect.

Non-finite values render as `0` so a scraper never chokes. Errors
from the snapshot path return HTTP 500 with a `# error: ...` comment
line so the scraper still receives a parseable response.

### Scrape config

```yaml
scrape_configs:
  - job_name: jnoccio_fusion
    metrics_path: /metrics
    scrape_interval: 15s
    static_configs:
      - targets: ['localhost:8088']  # adjust to your gateway bind
```

### Metric families

Gateway-wide (no labels):

| Name | Type | Meaning |
|---|---|---|
| `fusion_models_total` | gauge | Registered models. |
| `fusion_models_enabled` | gauge | Enabled (routable) models. |
| `fusion_requests_total` | counter | Total upstream requests across all models. |
| `fusion_success_total` | counter | Total successful upstream requests. |
| `fusion_failure_total` | counter | Total failed upstream requests. |
| `fusion_prompt_tokens_total` | counter | Total prompt tokens billed. |
| `fusion_completion_tokens_total` | counter | Total completion tokens billed. |
| `fusion_latency_avg_ms` | gauge | Call-count-weighted average upstream latency. |
| `fusion_agents_active` | gauge | Agents reporting heartbeats inside the active window. |
| `fusion_instances_total` | gauge | Managed gateway instances (main + spawned). |

Per-model series labeled `{model, provider}` (counters) or `{model}`
(gauges):

| Name | Type | Meaning |
|---|---|---|
| `fusion_model_requests_total` | counter | Per-model call count. |
| `fusion_model_success_total` | counter | Per-model success count. |
| `fusion_model_failure_total` | counter | Per-model failure count. |
| `fusion_model_win_total` | counter | Per-model fusion-sample win count. |
| `fusion_model_prompt_tokens_total` | counter | Per-model prompt tokens billed. |
| `fusion_model_completion_tokens_total` | counter | Per-model completion tokens billed. |
| `fusion_model_latency_avg_ms` | gauge | Per-model average latency. |
| `fusion_model_latency_last_ms` | gauge | Per-model latency of the most recent call. |
| `fusion_model_hourly_used` | gauge | Per-model rolling 1h request count. |
| `fusion_model_enabled` | gauge | Per-model enabled flag (1 = enabled, 0 = disabled). |

19 metric families total (10 gateway + 9 per-model).

### Grafana hint

The metric names above are stable. A dashboard typically wants:

- Top-row stat panels over `fusion_requests_total`,
  `fusion_success_total`, `fusion_failure_total`, and
  `fusion_latency_avg_ms`.
- A timeseries per-model error rate as
  `rate(fusion_model_failure_total[5m]) /
  rate(fusion_model_requests_total[5m])` grouped by
  `{model, provider}`.
- A spend / token graph from
  `rate(fusion_model_prompt_tokens_total[5m])` and
  `fusion_model_completion_tokens_total`.
- A capacity panel showing `fusion_model_hourly_used` vs your per-model
  cap.

A reference dashboard JSON is not shipped here on purpose — wire the
above names into whatever existing fleet dashboard you already run.
