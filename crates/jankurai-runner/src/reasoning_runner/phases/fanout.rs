//! Concurrent fanout helper for the parallel brainstorm phase.
//!
//! Gated behind `JEKKO_REASONING_PARALLEL=1` at the call site; this module
//! itself is policy-free. Drives all `cap` lanes concurrently on the calling
//! task via `futures::future::join_all`, then returns the results in
//! deterministic lane-index order so the caller's persistence loop preserves
//! the existing sequential SQLite/EventSink shape.
//!
//! Why not `tokio::task::JoinSet`? The brainstorm phase receives the model
//! client as a borrowed `&dyn ModelClient` from the orchestrator (whose public
//! signature is locked by callers outside this module's edit boundary), so
//! spawned tasks would need an owned `'static` clone we can't construct
//! without unsafe lifetime extension. `join_all` polls concurrently on the
//! same task — sufficient parallelism for I/O-bound model calls (each `await`
//! yields back to the scheduler so peer lanes can progress) and side-steps
//! the `Send + 'static` requirement entirely.
//!
//! Reducer fence: the next phase (`critique_phase`) reads lanes via SQL after
//! `brainstorm_phase` returns, so the serial persistence loop in the caller
//! gives us the fence implicitly. A `debug_assert` in `critique_phase`
//! double-checks the invariant.

use std::future::Future;

use anyhow::Result;
use futures::future::join_all;

/// Drive `cap` lanes concurrently via `spawn_lane`, await all of them, and
/// return results in lane-index order.
///
/// `spawn_lane(idx)` is called once per lane (0..cap) up front, then all
/// returned futures are polled concurrently on the calling task. Results are
/// paired with their lane index and sorted before being returned so the
/// caller can apply them deterministically.
pub(super) async fn run_lanes_parallel<F, Fut, T>(
    cap: usize,
    mut spawn_lane: F,
) -> Vec<(usize, Result<T>)>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut futures = Vec::with_capacity(cap);
    for idx in 0..cap {
        let fut = spawn_lane(idx);
        futures.push(async move {
            let result = fut.await;
            (idx, result)
        });
    }
    let mut results: Vec<(usize, Result<T>)> = join_all(futures).await;
    results.sort_by_key(|(idx, _)| *idx);
    results
}
