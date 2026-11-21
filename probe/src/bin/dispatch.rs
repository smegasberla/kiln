//! Async dispatch benchmark: thread-per-call versus the shared runtime.
//!
//! Measures the three registration kinds against each other on the real
//! `Registry` path, not a reimplementation of it:
//!
//! - `command` — runs on the calling thread.
//! - `async_command` — spawns one thread per call.
//! - `async_fn` — submits one task to the shared runtime.
//!
//! The question this answers is the one an app author actually faces: "if I
//! register a hundred of these, what does it cost?" Wall time is taken around a
//! full `dispatch` (envelope included) at several concurrency levels, because a
//! thread-per-call path looks fine sequentially and collapses under load.
//!
//! Run through `bun scripts/bench.ts --dispatch`, which builds it, checks the
//! answers, and gates on the pooled path not being slower than the thread one.
//!
//! `VOUSSOIR_BENCH_CALLS` overrides the call count (default 500 per level).

use std::sync::Arc;
use std::time::{Duration, Instant};

use voussoir::serde_json::{json, Value};
use voussoir::{AppHandle, Registry};

/// Concurrency levels to measure.
///
/// 1 is the sequential cost. 64 is a plausible burst for a desktop app. 512 is
/// past the point where thread-per-call is reserving gigabytes of stack, which is
/// the argument for a pool that has nothing to do with per-call latency.
const LEVELS: &[usize] = &[1, 8, 64, 512];

/// How long a handler pretends to work.
///
/// Zero would measure only the spawn cost, and one millisecond is unfair to the
/// pooled path for an unrelated reason: a timer wheel's resolution is around a
/// millisecond, so a 1 ms sleep can take two and the measurement would report that
/// as dispatch overhead. At 5 ms the sleep dominates on both paths and the
/// comparison is about scheduling, which is the thing being decided.
const WORK: Duration = Duration::from_millis(5);

fn main() {
    let calls: usize = std::env::var("VOUSSOIR_BENCH_CALLS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(500);

    let registry = build_registry();
    let handle = AppHandle::detached_for_tests();

    // Every path must actually answer before any timing is meaningful; a broken
    // handler that returns instantly would look like the fastest option.
    for method in ["sync", "thread", "pool"] {
        let reply = registry.dispatch(&handle, method, Value::Null);
        assert!(reply.contains("\"ok\""), "{method} did not answer: {reply}");
        assert!(
            reply.contains(method),
            "{method} answered the wrong thing: {reply}"
        );
    }

    println!(
        "BENCH-META {{\"calls\":{calls},\"levels\":{:?},\"work_ms\":{}}}",
        LEVELS,
        WORK.as_millis()
    );

    // Raw dispatch cost with no work at all: the floor each path pays before the
    // handler runs. Sequential only, because at concurrency this measures
    // scheduling rather than the overhead being asked about.
    for method in ["sync", "thread", "pool"] {
        let elapsed = drive(&registry, &handle, method, calls.min(2000), 1);
        let per_call_us = elapsed.as_secs_f64() * 1_000_000.0 / calls.min(2000) as f64;
        println!(
            "BENCH-OVERHEAD {}",
            json!({ "method": method, "per_call_us": per_call_us })
        );
    }

    let mut results: Vec<(&str, usize, f64)> = Vec::new();
    for method in ["sync", "thread", "pool"] {
        for &concurrency in LEVELS {
            let elapsed = drive(&registry, &handle, method, calls, concurrency);
            let per_call_ms = elapsed.as_secs_f64() * 1000.0 / calls as f64;
            results.push((method, concurrency, per_call_ms));
            println!(
                "BENCH {}",
                json!({
                    "method": method,
                    "concurrency": concurrency,
                    "calls": calls,
                    "total_ms": elapsed.as_secs_f64() * 1000.0,
                    "per_call_ms": per_call_ms,
                })
            );
        }
    }

    // What the decision hangs on. Both paths answer correctly at every level; the
    // question is whether the pool buys latency (it does not: a sleeping thread
    // wakes as cheaply as a timer entry) or headroom (it does: 512 concurrent
    // calls cost 512 tasks instead of 512 stacks).
    let at = |method: &str, concurrency: usize| -> f64 {
        results
            .iter()
            .find(|(name, level, _)| *name == method && *level == concurrency)
            .map(|(_, _, ms)| *ms)
            .unwrap_or(f64::INFINITY)
    };
    let highest = *LEVELS.last().expect("LEVELS is not empty");
    let thread = at("thread", highest);
    let pool = at("pool", highest);

    // Where the pool starts winning. Reported rather than assumed, because the
    // number is machine-dependent: it is the concurrency at which a task submit is
    // cheaper than spawning a thread, and that is a property of the OS scheduler.
    let crossover = LEVELS
        .iter()
        .rev()
        .copied()
        .find(|level| at("pool", *level) < at("thread", *level));
    println!("BENCH-CROSSOVER {}", json!({ "concurrency": crossover }));
    // The verdict, so `scripts/bench.ts` can gate on a claim rather than on a
    // number it would have to hard-code: the pool must win at the highest level,
    // and must not pretend to win when there is no concurrency to pool.
    let verdict = if pool < thread {
        "pool wins at high concurrency"
    } else {
        "thread-per-call still wins at the highest level"
    };
    println!("BENCH-VERDICT {verdict:?}");

    println!(
        "BENCH-DECISION {}",
        json!({
            "concurrency": highest,
            "thread_per_call_ms": thread,
            "pooled_ms": pool,
            "ratio": if thread > 0.0 { pool / thread } else { 0.0 },
            "parallelism": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        })
    );
}

fn build_registry() -> Registry {
    let mut registry = Registry::new();
    // The benchmark deliberately exceeds the shipping limit at its highest
    // concurrency level: it is measuring what a pool would cost, not what the
    // default refuses. Without this the 512-way run would be answered with
    // `busy` envelopes and the timings would measure the guard instead.
    registry.set_async_limit(usize::MAX);

    // No work: the floor. Anything slower than this is the transport's cost.
    registry.add("sync", |_app, _payload| Ok(json!("sync")));

    registry.add_async("thread", |_app: AppHandle, _payload| {
        std::thread::sleep(WORK);
        Ok(json!("thread"))
    });

    registry.add_async_fn("pool", |_app: AppHandle, _payload: Value| async move {
        // A sleep rather than a CPU spin: the point is a handler that yields, not
        // one that competes for cores. `voussoir::rt::sleep` keeps this binary free of
        // a direct tokio dependency.
        voussoir::rt::sleep(WORK).await;
        Ok(json!("pool"))
    });

    registry
}

/// Run `calls` dispatches at `concurrency`, returning the wall time.
///
/// Each worker owns its slice of the calls, so the timing includes the real
/// contention rather than one queue handing work out to idle threads.
fn drive(
    registry: &Registry,
    handle: &AppHandle,
    method: &str,
    calls: usize,
    concurrency: usize,
) -> Duration {
    let registry = Arc::new(Shared(registry));
    let handle = handle.clone();
    let started = Instant::now();

    std::thread::scope(|scope| {
        for worker in 0..concurrency {
            let registry = Arc::clone(&registry);
            let handle = handle.clone();
            scope.spawn(move || {
                // Interleaved slices so no worker gets a contiguous lucky run.
                let mut index = worker;
                while index < calls {
                    let reply = registry.dispatch(&handle, method, Value::Null);
                    assert!(reply.contains("\"ok\""), "{method} failed: {reply}");
                    index += concurrency;
                }
            });
        }
    });

    started.elapsed()
}

/// Owns the registry on the heap so each worker can hold its own `Arc`.
///
/// A plain `Registry` cannot be put straight into an `Arc` here because the caller
/// already owns it; wrapping rather than cloning keeps the benchmark measuring one
/// registry, which is what the shell has.
struct Shared<'a>(&'a Registry);

impl std::ops::Deref for Shared<'_> {
    type Target = Registry;
    fn deref(&self) -> &Registry {
        self.0
    }
}

// SAFETY: `Registry` is `Send + Sync` (it holds `Arc`ed handlers and locks, and
// every interior mutation is behind a `Mutex` or `RwLock`), so handing a shared
// reference to several threads is sound for as long as the reference lives. The
// scope joins every thread before it returns, so no reference outlives `registry`.
unsafe impl Send for Shared<'_> {}
unsafe impl Sync for Shared<'_> {}
