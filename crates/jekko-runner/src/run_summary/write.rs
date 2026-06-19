//! Write `summary.json` + `summary.md` next to the events file.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::types::RunSummary;

pub fn write_summary(run_dir: &Path, summary: &RunSummary) -> Result<()> {
    let json_path = run_dir.join("summary.json");
    let md_path = run_dir.join("summary.md");
    let json = serde_json::to_string_pretty(summary).context("serialize summary.json")?;
    fs::write(&json_path, json).with_context(|| format!("write {}", json_path.display()))?;
    fs::write(&md_path, render_markdown(summary))
        .with_context(|| format!("write {}", md_path.display()))?;
    Ok(())
}

/// Render the summary as a single-page markdown document for human readers.
pub fn render_markdown(s: &RunSummary) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str(&format!("# Run summary — `{}`\n\n", s.run_id));
    out.push_str(&format!("**Schema:** `{}`\n", s.schema_version));
    out.push_str(&format!("**Pipeline:** `{}`\n", s.pipeline));
    out.push_str(&format!("**Terminal status:** `{}`\n", s.terminal_status));
    if let Some(d) = s.duration_seconds {
        out.push_str(&format!("**Duration:** {} s\n\n", d));
    } else {
        out.push('\n');
    }

    if let Some(halt) = &s.halt_reason {
        out.push_str("## ⚠️ Halt reason\n\n");
        out.push_str(&format!("- **kind:** `{}`\n", halt.kind));
        if let Some(stage) = &halt.stage {
            out.push_str(&format!("- **stage:** `{}`\n", stage));
        }
        if let Some(n) = halt.consecutive_attempts {
            out.push_str(&format!("- **consecutive attempts:** {}\n", n));
        }
        if !halt.providers_tried.is_empty() {
            out.push_str(&format!(
                "- **providers tried:** {}\n",
                halt.providers_tried.join(", ")
            ));
        }
        if !halt.users_tried.is_empty() {
            out.push_str(&format!(
                "- **users tried:** {}\n",
                halt.users_tried.join(", ")
            ));
        }
        out.push_str(&format!("\n> {}\n\n", halt.summary));
    }

    out.push_str("## Pipeline progress\n\n");
    out.push_str(&format!(
        "- **deepest_stage:** `{}`\n",
        s.pipeline_progress.deepest_stage.as_deref().unwrap_or("—")
    ));
    out.push_str(&format!(
        "- **stages reached ({}):** {}\n",
        s.pipeline_progress.stages_reached.len(),
        s.pipeline_progress.stages_reached.join(", ")
    ));
    out.push_str(&format!(
        "- **stages completed ({}):** {}\n",
        s.pipeline_progress.stages_completed.len(),
        s.pipeline_progress.stages_completed.join(", ")
    ));
    out.push_str(&format!(
        "- **artifacts produced:** {}\n\n",
        s.pipeline_progress.artifacts_produced.join(", ")
    ));

    out.push_str("## Model calls\n\n");
    out.push_str(&format!(
        "- total_attempts: **{}** / parsed: **{}** / retryable_failures: {} / final_blocks: {} / empty_responses: {}\n",
        s.model_calls.total_attempts,
        s.model_calls.parsed_outcomes,
        s.model_calls.retryable_failures,
        s.model_calls.final_blocks,
        s.model_calls.empty_responses,
    ));
    out.push_str(&format!(
        "- latency p50: {} ms, p95: {} ms\n",
        s.model_calls
            .latency_p50_ms
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string()),
        s.model_calls
            .latency_p95_ms
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string()),
    ));
    out.push_str(&format!(
        "- by_user: {}\n",
        format_kv(&s.model_calls.by_user)
    ));
    out.push_str(&format!(
        "- by_provider: {}\n",
        format_kv(&s.model_calls.by_provider)
    ));
    out.push_str(&format!(
        "- by_kind: {}\n",
        format_kv(&s.model_calls.by_kind)
    ));
    out.push_str(&format!(
        "- by_state: {}\n",
        format_kv(&s.model_calls.by_state)
    ));
    if !s.model_calls.by_quality_band.is_empty() {
        out.push_str(&format!(
            "- by_quality_band: {}\n",
            format_kv(&s.model_calls.by_quality_band)
        ));
    }
    out.push('\n');

    out.push_str("## Budget\n\n");
    out.push_str(&format!(
        "- max_calls: {}, used: {}, remaining: {}, exhausted: {}\n\n",
        s.budget
            .max_calls
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string()),
        s.budget.used,
        s.budget
            .remaining
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string()),
        s.budget.exhausted,
    ));

    out.push_str("## Gates\n\n");
    if s.gates.is_empty() {
        out.push_str("none observed\n\n");
    } else {
        for (k, v) in &s.gates {
            out.push_str(&format!("- `{}`: **{}**\n", k, v));
        }
        out.push('\n');
    }

    out.push_str("## Signals\n\n");
    let fired: Vec<_> = s.signals_fired.iter().filter(|r| r.count > 0).collect();
    if fired.is_empty() {
        out.push_str("none fired\n\n");
    } else {
        out.push_str("| id | name | count |\n|---|---|---:|\n");
        for row in fired {
            out.push_str(&format!(
                "| `{}` | `{}` | {} |\n",
                row.id, row.name, row.count
            ));
        }
        out.push('\n');
    }

    if !s.operator_next_steps.is_empty() {
        out.push_str("## Operator next steps\n\n");
        for step in &s.operator_next_steps {
            out.push_str(&format!("- {}\n", step));
        }
        out.push('\n');
    }

    if !s.artifact_paths.is_empty() {
        out.push_str("## Artifact paths\n\n");
        for (k, v) in &s.artifact_paths {
            out.push_str(&format!("- `{}`: `{}`\n", k, v));
        }
        out.push('\n');
    }

    out.push_str("## Links\n\n");
    for (k, v) in &s.links {
        out.push_str(&format!("- [{}]({})\n", k, v));
    }
    out
}

fn format_kv(map: &std::collections::BTreeMap<String, u64>) -> String {
    if map.is_empty() {
        return "—".to_string();
    }
    map.iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join(", ")
}
