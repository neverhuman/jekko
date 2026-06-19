//! Path-keyed file lock map. Two layers:
//!
//!   1. In-process: `Arc<Mutex<HashMap<String, ()>>>` so concurrent fiber/task
//!      attempts to take overlapping paths queue safely.
//!   2. Cross-process: `.zyal/locks/<sha1(path)>.lock` files with the holder
//!      pid. Orphaned entries (pid not running) are reaped at acquire time.
//!
//! Locks are taken sorted-lexicographically with `try_lock_all_or_none`; if
//! any path is already held, we release everything we managed to grab and
//! return `LockAcquireOutcome::Conflict`. Sorting prevents any pair of callers
//! from deadlocking on a swapped pair of paths.

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use sha1::{Digest, Sha1};

#[derive(Debug, Clone)]
pub struct FileLockMap {
    inner: Arc<Mutex<HashMap<String, u32>>>,
    locks_root: PathBuf,
    pid: u32,
}

#[derive(Debug)]
pub enum LockAcquireOutcome {
    Acquired(LockGuard),
    Conflict { held_by: Vec<String> },
}

/// Drop-only guard. Releases all paths that were acquired in `try_lock_all`.
pub struct LockGuard {
    map: FileLockMap,
    paths: Vec<String>,
    released: bool,
}

impl std::fmt::Debug for LockGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockGuard")
            .field("paths", &self.paths)
            .field("released", &self.released)
            .finish()
    }
}

impl LockGuard {
    /// Explicitly release the guard early. Idempotent.
    pub fn release(mut self) {
        if !self.released {
            self.map.release_paths(&self.paths);
            self.released = true;
        }
    }

    pub fn paths(&self) -> &[String] {
        &self.paths
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if !self.released {
            self.map.release_paths(&self.paths);
            self.released = true;
        }
    }
}

impl FileLockMap {
    pub fn new(repo_root: &Path) -> Result<Self> {
        let locks_root = repo_root.join(".zyal/locks");
        fs::create_dir_all(&locks_root)
            .with_context(|| format!("mkdir -p {}", locks_root.display()))?;
        let map = Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            locks_root,
            pid: std::process::id(),
        };
        map.reap_orphaned()?;
        Ok(map)
    }

    /// Try to acquire all paths atomically. Returns `Conflict` with the names
    /// of paths that were already held if anything blocks; the caller should
    /// requeue the work onto the next wave.
    pub fn try_lock_all(&self, paths: &[String]) -> Result<LockAcquireOutcome> {
        let mut sorted: Vec<String> = paths.to_vec();
        sorted.sort();
        sorted.dedup();

        let mut acquired: Vec<String> = Vec::new();
        let mut conflicts: Vec<String> = Vec::new();

        let mut guard = self.inner.lock().expect("lock map poisoned");
        for path in &sorted {
            // In-process layer: any entry in the shared map means another
            // caller in this process (possibly via Arc::clone of this map)
            // holds it. The pid comparison would always equal `self.pid`
            // because we share a process, so we conflict on presence alone.
            if guard.contains_key(path) {
                conflicts.push(path.clone());
                continue;
            }
            // Cross-process layer: a `.zyal/locks/<sha1>.lock` file written by
            // a peer process at a *different* pid blocks us. Orphaned entries
            // (pid not alive) are reaped lazily inside the helper.
            if self.lock_file_held_by_other(path)? {
                conflicts.push(path.clone());
            }
        }
        conflicts.sort();
        conflicts.dedup();
        if !conflicts.is_empty() {
            return Ok(LockAcquireOutcome::Conflict { held_by: conflicts });
        }
        for path in &sorted {
            guard.insert(path.clone(), self.pid);
            self.write_lock_file(path)?;
            acquired.push(path.clone());
        }
        drop(guard);

        Ok(LockAcquireOutcome::Acquired(LockGuard {
            map: self.clone(),
            paths: acquired,
            released: false,
        }))
    }

    fn release_paths(&self, paths: &[String]) {
        let mut guard = self.inner.lock().expect("lock map poisoned");
        for path in paths {
            if guard.get(path).copied() == Some(self.pid) {
                guard.remove(path);
            }
            let _ = fs::remove_file(self.lock_file_for(path));
        }
    }

    pub fn locks_root(&self) -> &Path {
        &self.locks_root
    }

    fn lock_file_for(&self, path: &str) -> PathBuf {
        let mut hasher = Sha1::new();
        hasher.update(path.as_bytes());
        let digest = hasher.finalize();
        self.locks_root.join(format!("{:x}.lock", digest))
    }

    fn write_lock_file(&self, path: &str) -> Result<()> {
        let file = self.lock_file_for(path);
        fs::write(&file, format!("{}\n{}\n", self.pid, path))
            .with_context(|| format!("write lock {}", file.display()))?;
        Ok(())
    }

    fn lock_file_held_by_other(&self, path: &str) -> Result<bool> {
        let file = self.lock_file_for(path);
        let text = match fs::read_to_string(&file) {
            Ok(t) => t,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err).with_context(|| format!("read lock {}", file.display()));
            }
        };
        let pid_line = text.lines().next().unwrap_or("0");
        let pid: u32 = pid_line.trim().parse().unwrap_or(0);
        if pid == self.pid {
            return Ok(false);
        }
        if !pid_is_alive(pid) {
            let _ = fs::remove_file(&file);
            return Ok(false);
        }
        Ok(true)
    }

    fn reap_orphaned(&self) -> Result<()> {
        let entries = match fs::read_dir(&self.locks_root) {
            Ok(rd) => rd,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err).context("read .zyal/locks"),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let text = match fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let pid_line = text.lines().next().unwrap_or("0");
            let pid: u32 = pid_line.trim().parse().unwrap_or(0);
            if pid != 0 && !pid_is_alive(pid) {
                let _ = fs::remove_file(&path);
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: Direct libc kill via the system C library. The FFI signature
    // matches `kill(2)` exactly: `int kill(pid_t pid, int sig)` with
    // `pid_t == i32` on every supported unix target. Signal 0 is a POSIX
    // existence probe (the syscall only validates that `pid` resolves to a
    // process the caller can signal); no memory is read or written and no
    // resources are owned by this call, so the unsafe surface is strictly the
    // C ABI itself. Returns 0 iff the pid is alive, -1 with errno=ESRCH if not.
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // SAFETY: see the block comment above the extern declaration.
    unsafe { kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
    // Conservative default on non-unix: assume alive so we never reap a real
    // peer. Orphaned files only matter for crash recovery on the same host.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn acquires_disjoint_paths() {
        let dir = tempdir().unwrap();
        let map = FileLockMap::new(dir.path()).unwrap();
        let outcome = map
            .try_lock_all(&["src/a.rs".into(), "src/b.rs".into()])
            .unwrap();
        match outcome {
            LockAcquireOutcome::Acquired(g) => assert_eq!(g.paths().len(), 2),
            LockAcquireOutcome::Conflict { .. } => panic!("expected acquired"),
        }
    }

    #[test]
    fn second_caller_sees_conflict_then_succeeds_after_release() {
        let dir = tempdir().unwrap();
        let map = FileLockMap::new(dir.path()).unwrap();
        let first = map.try_lock_all(&["x".into(), "y".into()]).unwrap();
        let first = match first {
            LockAcquireOutcome::Acquired(g) => g,
            _ => panic!("expected acquired"),
        };

        let map2 = map.clone();
        let conflict = map2.try_lock_all(&["y".into(), "z".into()]).unwrap();
        match conflict {
            LockAcquireOutcome::Conflict { held_by } => assert_eq!(held_by, vec!["y".to_string()]),
            _ => panic!("expected conflict"),
        }

        first.release();
        let outcome = map2.try_lock_all(&["y".into(), "z".into()]).unwrap();
        match outcome {
            LockAcquireOutcome::Acquired(g) => assert_eq!(g.paths().len(), 2),
            _ => panic!("expected acquired after release"),
        }
    }

    #[test]
    fn duplicate_paths_in_request_are_normalized() {
        let dir = tempdir().unwrap();
        let map = FileLockMap::new(dir.path()).unwrap();
        let outcome = map
            .try_lock_all(&["a".into(), "a".into(), "b".into()])
            .unwrap();
        match outcome {
            LockAcquireOutcome::Acquired(g) => {
                assert_eq!(g.paths(), &["a".to_string(), "b".to_string()])
            }
            _ => panic!("expected acquired"),
        }
    }

    #[test]
    fn orphaned_pid_lock_file_is_reaped_and_lock_succeeds() {
        let dir = tempdir().unwrap();
        let locks_dir = dir.path().join(".zyal/locks");
        fs::create_dir_all(&locks_dir).unwrap();
        // Write a lock file referencing pid 1 (init) using a fake path; init
        // is alive on every unix box. We need a *orphaned* example, so we use
        // pid 999999 which is virtually guaranteed not to exist.
        let mut hasher = Sha1::new();
        hasher.update(b"src/orphaned.rs");
        let digest = hasher.finalize();
        let orphaned_file = locks_dir.join(format!("{:x}.lock", digest));
        fs::write(&orphaned_file, "999999\nsrc/orphaned.rs\n").unwrap();

        let map = FileLockMap::new(dir.path()).unwrap();
        let outcome = map.try_lock_all(&["src/orphaned.rs".into()]).unwrap();
        assert!(matches!(outcome, LockAcquireOutcome::Acquired(_)));
    }

    #[test]
    fn drop_releases_lock() {
        let dir = tempdir().unwrap();
        let map = FileLockMap::new(dir.path()).unwrap();
        {
            let _g = match map.try_lock_all(&["x".into()]).unwrap() {
                LockAcquireOutcome::Acquired(g) => g,
                _ => panic!("expected acquired"),
            };
            // _g lives for the block.
        }
        let outcome = map.try_lock_all(&["x".into()]).unwrap();
        assert!(matches!(outcome, LockAcquireOutcome::Acquired(_)));
    }
}
