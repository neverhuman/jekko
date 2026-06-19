# Domain Observability Contract

This file defines a typed, local-first repair surface for domain failures.

## Repair receipt

```json
{
  "rule_id": "OBS-001",
  "docs_url": "docs/testing.md#observability-and-repair-receipts",
  "repair_hint": "rerun `just score` after the scoped domain change",
  "rerun_command": "just score",
  "artifact_paths": [
    "target/jankurai/repo-score.json",
    "target/jankurai/repo-score.md",
    "target/jankurai/repair-queue.jsonl",
    "target/jankurai/score-history.jsonl",
    "target/jankurai/jankurai.sarif",
    "target/jankurai/summary.md"
  ],
  "result_code": "pass",
  "purpose": "typed agent-friendly exception surface",
  "reason": "opaque failures slow local debugging and reruns",
  "common fixes": [
    "add a typed error variant with purpose, reason, docs_url, and repair_hint",
    "attach the failing proof lane and receipt artifact to the repair queue"
  ],
  "evidence_path": "crates/domain/observability.md",
  "timestamp_utc": "2026-05-07T10:42:25Z"
}
```
