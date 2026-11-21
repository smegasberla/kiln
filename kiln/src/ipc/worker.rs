//! The bound on thread-per-call handlers.
//!
//! A permit is taken before the thread spawns and released on drop, so a handler
//! that returns, unwinds, or fails at spawn all free their slot the same way. That
//! is what keeps the count from drifting upward and silently converging on
//! "everything is busy".

use std::sync::atomic::{AtomicUsize, Ordering};

/// How many worker-thread commands may run at once by default.
///
/// Tied to the machine rather than a constant: a four-core laptop and a 32-core
/// workstation should not refuse the same burst. The floor of four keeps the
/// bound from serialising ordinary I/O on a small target, and there is no upper
/// cap because the limit is only a memory guard, not a throttle.
pub(super) fn default_worker_limit() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .max(4)
}

/// A worker slot, released on drop.
///
/// Drop rather than an explicit release is what makes the bound honest: a
/// handler that returns, unwinds, or fails at thread spawn all free their slot
/// the same way, so the count cannot drift upward and silently converge on
/// "everything is busy".
pub(super) struct WorkerSlot(std::sync::Arc<AtomicUsize>);

impl Drop for WorkerSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Take a worker slot, or `None` when every slot is already in use.
///
/// A compare-exchange loop rather than a mutex: the counter is touched once at
/// the start of a command and once at the end, and a contended atomic is far
/// cheaper than a contended lock on a path that is meant to be spawned hundreds
/// of times at once.
pub(super) fn claim_worker_slot(inflight: &std::sync::Arc<AtomicUsize>, limit: usize) -> Option<WorkerSlot> {
    let mut current = inflight.load(Ordering::SeqCst);
    loop {
        if current >= limit {
            return None;
        }
        match inflight.compare_exchange_weak(
            current,
            current + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return Some(WorkerSlot(std::sync::Arc::clone(inflight))),
            Err(observed) => current = observed,
        }
    }
}
