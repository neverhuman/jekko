use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

use jekko_runner::hero_judge::{HeroJudgeRunSummary, HeroJudgeRunbook};
use jekko_runner::hero_judge_runner::run_hero_judge_run;

use super::super::cli::HeroJudgeRunArgs;
use super::client::hero_judge_client;

pub(super) async fn run_series_trials(
    repo: &Path,
    series_id: &str,
    args: &HeroJudgeRunArgs,
    runbook: HeroJudgeRunbook,
) -> Result<Vec<HeroJudgeRunSummary>> {
    let parallelism = series_parallelism(args.runs);
    if parallelism == 1 {
        let mut runs = Vec::new();
        for trial in 1..=args.runs {
            let child_run_id = format!("{series_id}-trial-{trial:03}");
            let client = hero_judge_client(args, &runbook);
            runs.push(
                run_hero_judge_run(
                    repo,
                    &child_run_id,
                    &args.zyal,
                    runbook.clone(),
                    args.max_generations,
                    args.live,
                    client.as_ref(),
                )
                .await?,
            );
        }
        return Ok(runs);
    }

    let mut next_trial = 1;
    let mut completed = Vec::with_capacity(args.runs);
    let mut children: Vec<SeriesChild> = Vec::new();
    while next_trial <= args.runs || !children.is_empty() {
        while next_trial <= args.runs && children.len() < parallelism {
            let trial = next_trial;
            let child_run_id = format!("{series_id}-trial-{trial:03}");
            children.push(spawn_series_child(repo, &child_run_id, trial, args)?);
            next_trial += 1;
        }
        let mut finished = None;
        for (idx, child) in children.iter_mut().enumerate() {
            if child
                .child
                .try_wait()
                .with_context(|| format!("poll trial {}", child.trial))?
                .is_some()
            {
                finished = Some(idx);
                break;
            }
        }
        if let Some(idx) = finished {
            let child = children.swap_remove(idx);
            completed.push(read_series_child(child)?);
        } else {
            thread::sleep(Duration::from_millis(250));
        }
    }
    completed.sort_by_key(|(trial, _)| *trial);
    Ok(completed.into_iter().map(|(_, summary)| summary).collect())
}

struct SeriesChild {
    trial: usize,
    child: Child,
}

fn spawn_series_child(
    repo: &Path,
    child_run_id: &str,
    trial: usize,
    args: &HeroJudgeRunArgs,
) -> Result<SeriesChild> {
    let mut command = Command::new(std::env::current_exe().context("resolve current exe")?);
    command
        .arg("--repo")
        .arg(repo)
        .arg("--run-id")
        .arg(child_run_id)
        .arg("hero-judge-run")
        .arg("--zyal")
        .arg(&args.zyal);
    if args.live {
        command.arg("--live");
    }
    if let Some(provider) = args.provider.as_deref() {
        command.arg("--provider").arg(provider);
    }
    if let Some(model) = args.model.as_deref() {
        command.arg("--model").arg(model);
    }
    if let Some(max_generations) = args.max_generations {
        command
            .arg("--max-generations")
            .arg(max_generations.to_string());
    }
    let child = command
        .envs(series_child_env(trial))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn trial {trial}"))?;
    Ok(SeriesChild { trial, child })
}

fn series_child_env(trial: usize) -> Vec<(&'static str, PathBuf)> {
    let Some(db) = std::env::var_os("JEKKO_DB").map(PathBuf::from) else {
        return Vec::new();
    };
    let child_db = match db.extension().and_then(|extension| extension.to_str()) {
        Some(extension) => db.with_extension(format!("trial-{trial:03}.{extension}")),
        None => db.with_file_name(format!("{}.trial-{trial:03}", db.display())),
    };
    vec![("JEKKO_DB", child_db)]
}

fn read_series_child(child: SeriesChild) -> Result<(usize, HeroJudgeRunSummary)> {
    let output = child
        .child
        .wait_with_output()
        .with_context(|| format!("wait trial {}", child.trial))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("trial {} failed: {}", child.trial, stderr.trim());
    }
    let summary: HeroJudgeRunSummary = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("decode trial {} summary", child.trial))?;
    Ok((child.trial, summary))
}

fn series_parallelism(run_count: usize) -> usize {
    std::env::var("HERO_JUDGE_SERIES_PARALLEL")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1)
        .min(run_count.max(1))
        .min(12)
}
