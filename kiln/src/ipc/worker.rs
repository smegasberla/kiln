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
pub(super) fn claim_worker_slot(
    inflight: &std::sync::Arc<AtomicUsize>,
    limit: usize,
) -> Option<WorkerSlot> {
    if reserve(inflight, limit) {
        Some(WorkerSlot(std::sync::Arc::clone(inflight)))
    } else {
        None
    }
}

/// Take one of `limit` places in `counter`, or report that none is free.
///
/// Shared by both guards below so the admission check and the thread check
/// cannot drift into disagreeing about what "full" means.
pub(super) fn reserve(counter: &AtomicUsize, limit: usize) -> bool {
    let mut current = counter.load(Ordering::SeqCst);
    loop {
        if current >= limit {
            return false;
        }
        match counter.compare_exchange_weak(
            current,
            current + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

/// How many async requests an app may have outstanding at once.
///
/// The worker limit bounds threads, which is not a bound on *load*: a page that
/// loops `invoke` on a command that finishes quickly keeps every slot busy for
/// as long as it likes, so the count of requests the shell will accept is set by
/// the caller and not by the shell. This counter bounds the total, so the shell
/// refuses at a number it chose rather than at the rate the page asks.
///
/// It is checked before a thread is claimed, so a refusal past this line costs
/// an envelope and no stack. The default matches the worker limit because that
/// is the behaviour every existing app has; the point of the knob is that a
/// deployment can set it lower and be strict about it.
pub(super) fn default_max_pending(limit: usize) -> usize {
    limit.max(1)
}

/// Admission to the async path, released when the reply is sent.
pub(super) struct Admission(std::sync::Arc<AtomicUsize>);

impl Drop for Admission {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Count one outstanding async request, or `None` when the cap is reached.
pub(super) fn claim_admission(
    pending: &std::sync::Arc<AtomicUsize>,
    max: usize,
) -> Option<Admission> {
    if reserve(pending, max) {
        Some(Admission(std::sync::Arc::clone(pending)))
    } else {
        None
    }
}
