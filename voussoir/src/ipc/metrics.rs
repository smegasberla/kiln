//! Per-command latency, kept only while diagnostics are on.
//!
//! Recording is free-standing rather than a registry method so an async dispatch
//! can record from its worker thread without borrowing the registry, and so the
//! sync and async paths cannot drift into measuring different things.

use std::collections::HashMap;

/// How many latency samples to keep per command.
///
/// Bounded because a dev session can call one command a million times; a
/// percentile over the last thousand calls answers "is this slow" just as well
/// as over all of them.
const LATENCY_SAMPLES: usize = 1_000;

/// A dispatch at or above this many milliseconds is recorded by name.
///
/// 50 ms is three frames at 60 Hz: below it a command is invisible to the user,
/// above it a window visibly hitches.
const SLOW_COMMAND_MS: u64 = 50;

/// Record one dispatch against a shared sample map.
///
/// Free-standing so the async path can call it from a worker thread that does
/// not have the registry, and so the sync path and the async path cannot drift
/// into measuring different things.
pub(super) fn record_latency(
    samples: &std::sync::Mutex<HashMap<String, Vec<u64>>>,
    method: &str,
    elapsed: std::time::Duration,
) {
    if !crate::diagnostics::enabled() {
        return;
    }
    let millis = elapsed.as_millis() as u64;
    {
        let mut samples = samples
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let slot = samples.entry(method.to_owned()).or_default();
        if slot.len() == LATENCY_SAMPLES {
            slot.remove(0);
        }
        slot.push(millis);
    }
    // One line per slow call, which is the one a developer acts on; the aggregate
    // comes from `latencies()` rather than flooding stderr on every dispatch.
    if millis >= SLOW_COMMAND_MS {
        crate::diagnostics::record(
            crate::diagnostics::Kind::Dispatch,
            "slow command",
            serde_json::json!({ "method": method, "ms": millis }),
        );
    }
}

impl super::registry::Registry {
    /// Latency totals per command, for the diagnostics report.
    ///
    /// Only populated while diagnostics are on, so a release build allocates
    /// nothing here. The map is per registry, which is per app, so two apps in
    /// one process (a test harness, say) do not share numbers.
    pub fn latencies(&self) -> Vec<(String, crate::diagnostics::Summary)> {
        let samples = self
            .latency
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut out: Vec<(String, crate::diagnostics::Summary)> = samples
            .iter()
            .map(|(method, times)| (method.clone(), crate::diagnostics::Summary::of(times)))
            .collect();
        // Slowest mean first: the list is read to find the command to fix.
        out.sort_by_key(|left| std::cmp::Reverse(left.1.mean_ms()));
        out
    }

    /// Every command's latency as JSON, slowest first.
    ///
    /// This is what `voussoir dev --verbose` prints when the app exits: the map is
    /// already sorted by mean, so the first lines are the commands to look at.
    pub fn latency_report(&self) -> serde_json::Value {
        let entries: Vec<serde_json::Value> = self
            .latencies()
            .into_iter()
            .map(|(method, summary)| {
                let mut value = summary.to_json();
                value["method"] = serde_json::Value::String(method);
                value
            })
            .collect();
        serde_json::Value::Array(entries)
    }

    /// Drop the latency samples, so a dev session can measure one scenario.
    pub fn clear_latencies(&self) -> usize {
        let mut samples = self
            .latency
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count: usize = samples.values().map(Vec::len).sum();
        samples.clear();
        count
    }

    pub(super) fn note_latency(&self, method: &str, elapsed: std::time::Duration) {
        record_latency(&self.latency, method, elapsed);
    }
}
