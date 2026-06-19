use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;

use anyhow::{Context, Result};
use jekko_runner::events::{Event, EventKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::{emit_tick, TickState, WatchArgs, WatchFormat, DEBOUNCE};

pub(super) fn follow(
    path: &Path,
    mut offset: u64,
    state: &mut TickState,
    args: &WatchArgs,
    format: WatchFormat,
) -> Result<()> {
    // Watch the *parent* directory so we still receive events if the file
    // doesn't exist yet (notify can't subscribe to a missing path).
    let watch_dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    if !watch_dir.exists() {
        std::fs::create_dir_all(&watch_dir)
            .with_context(|| format!("mkdir -p {}", watch_dir.display()))?;
    }

    let (tx, rx) = channel::<notify::Result<notify::Event>>();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .context("create file watcher")?;
    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("watch {}", watch_dir.display()))?;

    loop {
        // Wait for a change. We also wake every 5s so stall detection can
        // fire even when there's no fs activity.
        let timeout = Duration::from_secs(5);
        match rx.recv_timeout(timeout) {
            Ok(Ok(_event)) => {
                // Coalesce burst of notifications within the debounce window
                // before reading the file.
                let deadline = std::time::Instant::now() + DEBOUNCE;
                while let Ok(remaining) = deadline
                    .checked_duration_since(std::time::Instant::now())
                    .ok_or(())
                {
                    match rx.recv_timeout(remaining) {
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
            }
            Ok(Err(err)) => {
                eprintln!("watch error: {err}");
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {
                // Periodic tick so stall rules still fire on a quiet stream.
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }

        let (new_events, new_offset) = read_from_offset(path, offset)?;
        offset = new_offset;
        emit_tick(&new_events, state, args, format, /* initial */ false)?;

        if state
            .all_events
            .iter()
            .any(|ev| matches!(ev.kind, EventKind::RunFinished))
        {
            break;
        }
    }
    Ok(())
}

/// Read any new lines appended past `offset` and return the parsed events
/// plus the new offset. Lines that fail to parse are skipped (with a stderr
/// notice) so a single malformed event doesn't kill the watcher.
pub(super) fn read_from_offset(path: &Path, offset: u64) -> Result<(Vec<Event>, u64)> {
    if !path.exists() {
        return Ok((Vec::new(), 0));
    }
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let len = file.metadata()?.len();
    // File was rotated / truncated - restart from the beginning.
    let read_from = if offset > len { 0 } else { offset };
    file.seek(SeekFrom::Start(read_from))?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    let mut consumed = read_from;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(err) => {
                eprintln!("read {}: {err}", path.display());
                break;
            }
        };
        // +1 for the newline that BufRead stripped.
        consumed = consumed.saturating_add(line.len() as u64 + 1);
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(&line) {
            Ok(ev) => events.push(ev),
            Err(err) => {
                eprintln!("skip malformed event line: {err}");
            }
        }
    }
    // Clamp consumed to the real file length so we don't drift past EOF if
    // the last line had no trailing newline.
    let consumed = consumed.min(len);
    Ok((events, consumed))
}
