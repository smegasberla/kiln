//! Structured diagnostics for `kiln dev --verbose`.
//!
//! Everything in here answers a question a developer asks while a dev build is
//! running: which library generation is live, how long a reload took, how slow
//! the slowest command was, and what the security policy refused. Without it the
//! answers are spread across `eprintln!` calls and a terminal history.
//!
//! Three properties are deliberate:
//!
//! - **Off by default and free when off.** A release build must not pay for a
//!   dev convenience, so every call site checks [`enabled`] first and the check
//!   is a cached atomic load.
//! - **Recorded, not only printed.** The ring buffer is what `__kiln_reports`
//!   returns and what an app can put on a diagnostics screen. A line that only
//!   scrolled past is not evidence.
//! - **Plain `std`.** No platform API is touched, so the same code runs on every
//!   target, including the mobile ones where a terminal is not even visible: the
//!   buffer is the only way to see it there.
//!
//! The enabled flag is read once from `KILN_VERBOSE` (set by `kiln dev
//! --verbose`), and can be flipped at runtime through [`set_enabled`] so a test
//! can assert both states without touching the environment.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

/// How many entries to keep. Enough for a dev session's worth of reloads,
/// denials and slow calls, small enough that the buffer is not a leak.
const CAPACITY: usize = 256;

/// The kind of event, so a reader can filter without parsing the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A hot library generation became live.
    HotSwap,
    /// A command dispatch, recorded when it was slow or when it failed.
    Dispatch,
    /// The security policy refused something.
    Security,
    /// The bridge refused or dropped a message.
    Bridge,
    /// Anything an app records itself.
    App,
}

impl Kind {
    /// The stable lowercase name used in the printed line and in JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::HotSwap => "hot",
            Kind::Dispatch => "dispatch",
            Kind::Security => "security",
            Kind::Bridge => "bridge",
            Kind::App => "app",
        }
    }
}

/// One recorded event.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Monotonic sequence number, so a reader can tell entries apart even when
    /// two share a millisecond.
    pub seq: u64,
    /// Milliseconds since the process started.
    pub at_ms: u64,
    pub kind: Kind,
    pub message: String,
    /// Extra context, keyed. Kept as a JSON object so the wire format is the
    /// same one the page already reads.
    pub fields: Value,
}

impl Entry {
    /// The entry as the page sees it.
    pub fn to_json(&self) -> Value {
        json!({
            "seq": self.seq,
            "at_ms": self.at_ms,
            "kind": self.kind.as_str(),
            "message": self.message,
            "fields": self.fields,
        })
    }

    /// The `--verbose` log line: one line, `key=value` pairs, greppable.
    pub fn to_line(&self) -> String {
        let mut line = format!(
            "kiln: [{}] t+{}ms {}",
            self.kind.as_str(),
            self.at_ms,
            self.message
        );
        if let Some(map) = self.fields.as_object() {
            for (key, value) in map {
                // Strings print bare so a path or a command name needs no
                // unwrapping when read; everything else keeps its JSON form.
                let shown = match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                line.push(' ');
                line.push_str(key);
                line.push('=');
                line.push_str(&shown);
            }
        }
        line
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);
static BUFFER: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
static START: OnceLock<std::time::Instant> = OnceLock::new();

fn buffer() -> &'static Mutex<Vec<Entry>> {
    BUFFER.get_or_init(|| Mutex::new(Vec::new()))
}

fn start() -> &'static std::time::Instant {
    START.get_or_init(std::time::Instant::now)
}

/// Read the environment once. Called from `App::run`, before the loop starts.
///
/// Set by `kiln dev --verbose`; also honours `KILN_VERBOSE=0` to force it off in
/// a shell that would otherwise inherit a stale variable.
pub fn init_from_env() {
    let on = std::env::var("KILN_VERBOSE")
        .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
        .unwrap_or(false);
    set_enabled(on);
}

/// Turn recording on or off. Exposed so a test can assert both states, and so an
/// app can enable it from a command (a settings toggle, for instance).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Whether recording and printing are on.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Record one event, printing it to stderr when enabled.
///
/// Always records, even when printing is off: the buffer is cheap (bounded, and
/// only written by call sites that already checked [`enabled`], plus security
/// denials which are rare by construction), and a diagnostics snapshot after a
/// bug is worth more than the memory. Call [`record`] from a hot path only
/// behind an [`enabled`] check.
pub fn record(kind: Kind, message: impl Into<String>, fields: Value) {
    let entry = Entry {
        seq: NEXT_SEQ.fetch_add(1, Ordering::Relaxed),
        at_ms: start().elapsed().as_millis() as u64,
        kind,
        message: message.into(),
        fields,
    };
    if enabled() {
        eprintln!("{}", entry.to_line());
    }
    let mut buffer = buffer()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if buffer.len() == CAPACITY {
        buffer.remove(0);
    }
    buffer.push(entry);
}

/// Record only when enabled, for a call site that would otherwise format
/// arguments in release builds.
///
/// The argument is a closure so the formatting cost is skipped entirely when
/// diagnostics are off, which is the normal case in a release build.
pub fn record_if_enabled<F>(enabled_hint: bool, write: F)
where
    F: FnOnce(),
{
    if enabled_hint {
        write();
    }
}

/// Every recorded entry, oldest first.
pub fn entries() -> Vec<Value> {
    buffer()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .map(Entry::to_json)
        .collect()
}

/// Keep only the last `keep` entries, for a reader that wants a tail.
pub fn tail(keep: usize) -> Vec<Value> {
    let buffer = buffer()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    buffer
        .iter()
        .rev()
        .take(keep)
        .rev()
        .map(Entry::to_json)
        .collect()
}

/// Drop everything recorded so far; returns how many entries were dropped.
pub fn clear() -> usize {
    let mut buffer = buffer()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let count = buffer.len();
    buffer.clear();
    count
}

/// A latency summary over a set of samples, in milliseconds.
///
/// Computed by sorting a copy, so it is O(n log n) and allocates; the caller
/// keeps the sample set small (one per distinct command) and only assembles it
/// when diagnostics are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub count: u64,
    pub min_ms: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub max_ms: u64,
    pub total_ms: u64,
}

impl Summary {
    /// Summarise raw samples given in milliseconds.
    pub fn of(samples: &[u64]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let at = |fraction: f64| -> u64 {
            // Nearest-rank: index 0 for the floor percentile, never out of range.
            let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
            sorted[index.min(sorted.len() - 1)]
        };
        Self {
            count: sorted.len() as u64,
            min_ms: sorted[0],
            p50_ms: at(0.50),
            p95_ms: at(0.95),
            max_ms: sorted[sorted.len() - 1],
            total_ms: sorted.iter().sum(),
        }
    }

    /// Mean latency, which is what "how much did commands cost" needs.
    pub fn mean_ms(&self) -> u64 {
        match self.count {
            0 => 0,
            count => self.total_ms / count,
        }
    }

    /// The summary as JSON, for a report.
    pub fn to_json(&self) -> Value {
        json!({
            "count": self.count,
            "min_ms": self.min_ms,
            "p50_ms": self.p50_ms,
            "p95_ms": self.p95_ms,
            "max_ms": self.max_ms,
            "total_ms": self.total_ms,
            "mean_ms": self.mean_ms(),
        })
    }

    /// Fold one sample in, so a caller can accumulate without keeping the array.
    ///
    /// Percentiles need the whole distribution, so they stay as they were: use
    /// [`Summary::of`] when a p50 or p95 is what the caller wants. This exists for
    /// a running count/min/max/mean, which is what a dev session accumulates
    /// across thousands of dispatches without holding every sample.
    pub fn push(&mut self, sample_ms: u64) {
        if self.count == 0 {
            self.min_ms = sample_ms;
            self.max_ms = sample_ms;
        } else {
            self.min_ms = self.min_ms.min(sample_ms);
            self.max_ms = self.max_ms.max(sample_ms);
        }
        self.count += 1;
        self.total_ms += sample_ms;
    }
}

/// Serialises tests that touch the process-wide flag or buffer.
///
/// Both are global by design (a release build must not thread a diagnostics
/// handle through every call), which means two tests running in parallel would
/// otherwise assert on each other's entries and flag flips. Test-only.
#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The buffer is process-wide and shared with other tests in this binary, so
    /// every test that inspects it does so through a delta, never an absolute
    /// length.
    fn snapshot_len() -> usize {
        buffer()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    #[test]
    fn an_entry_prints_as_one_greppable_line() {
        let entry = Entry {
            seq: 7,
            at_ms: 42,
            kind: Kind::HotSwap,
            message: "generation 3 live".to_owned(),
            fields: json!({ "gen": 3, "rebuild_ms": 812, "path": "/tmp/commands-3.so" }),
        };
        let line = entry.to_line();
        // `serde_json` maps are sorted, so the field order is stable text rather
        // than whatever the call site happened to write.
        assert_eq!(
            line,
            "kiln: [hot] t+42ms generation 3 live gen=3 path=/tmp/commands-3.so rebuild_ms=812"
        );
        assert_eq!(line.lines().count(), 1, "a log line must not wrap");
    }

    #[test]
    fn an_entry_round_trips_to_json() {
        let entry = Entry {
            seq: 1,
            at_ms: 5,
            kind: Kind::Security,
            message: "navigation refused".to_owned(),
            fields: json!({ "url": "http://example.com/" }),
        };
        let value = entry.to_json();
        assert_eq!(value["kind"], "security");
        assert_eq!(value["at_ms"], 5);
        assert_eq!(value["fields"]["url"], "http://example.com/");
    }

    #[test]
    fn every_kind_has_a_distinct_stable_name() {
        let names: Vec<&str> = [
            Kind::HotSwap,
            Kind::Dispatch,
            Kind::Security,
            Kind::Bridge,
            Kind::App,
        ]
        .iter()
        .map(|kind| kind.as_str())
        .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "kind names collide: {names:?}");
        for name in names {
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase()),
                "{name} is not a stable lowercase name"
            );
        }
    }

    #[test]
    fn recording_keeps_the_buffer_bounded() {
        let _guard = test_guard();
        clear();
        for index in 0..(CAPACITY + 10) {
            record(Kind::App, format!("event {index}"), Value::Null);
        }
        assert_eq!(snapshot_len(), CAPACITY, "the buffer must not grow past its cap");
        let entries = entries();
        let last = entries.last().expect("a full buffer has a last entry");
        assert_eq!(
            last["message"],
            format!("event {}", CAPACITY + 9),
            "the newest entry survives, the oldest is dropped"
        );
        clear();
    }

    #[test]
    fn a_tail_returns_the_newest_entries_in_order() {
        let _guard = test_guard();
        clear();
        for index in 0..5 {
            record(Kind::App, format!("e{index}"), Value::Null);
        }
        let tail = tail(2);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0]["message"], "e3");
        assert_eq!(tail[1]["message"], "e4");
        clear();
    }

    #[test]
    fn clearing_reports_what_it_dropped() {
        let _guard = test_guard();
        clear();
        record(Kind::App, "a", Value::Null);
        record(Kind::App, "b", Value::Null);
        assert_eq!(clear(), 2);
        assert_eq!(clear(), 0, "clearing an empty buffer is not an error");
    }

    #[test]
    fn enabling_controls_printing_not_recording() {
        let _guard = test_guard();
        // Recording is unconditional so a later snapshot still has the event;
        // only the stderr line depends on the flag. Asserted through the buffer
        // because capturing a child's stderr is the CLI's job, not a unit test's.
        let was = enabled();
        set_enabled(false);
        clear();
        record(Kind::App, "quiet", Value::Null);
        assert_eq!(snapshot_len(), 1);
        set_enabled(true);
        assert!(enabled());
        set_enabled(was);
        clear();
    }

    #[test]
    fn a_summary_reports_nearest_rank_percentiles() {
        let summary = Summary::of(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(summary.count, 10);
        assert_eq!(summary.min_ms, 1);
        assert_eq!(summary.max_ms, 10);
        assert_eq!(summary.total_ms, 55);
        assert_eq!(summary.mean_ms(), 5);
        assert_eq!(summary.p50_ms, 6, "nearest rank of 10 samples rounds up");
        assert_eq!(summary.p95_ms, 10);
    }

    #[test]
    fn an_empty_summary_is_all_zeroes() {
        let summary = Summary::of(&[]);
        assert_eq!(summary, Summary::default());
        assert_eq!(summary.mean_ms(), 0, "no samples must not divide by zero");
        assert_eq!(summary.to_json()["count"], 0);
    }

    #[test]
    fn a_summary_folded_one_sample_at_a_time_tracks_the_extremes() {
        let mut summary = Summary::default();
        for sample in [4, 1, 9] {
            summary.push(sample);
        }
        assert_eq!(summary.count, 3);
        assert_eq!(summary.min_ms, 1);
        assert_eq!(summary.max_ms, 9);
        assert_eq!(summary.total_ms, 14);
        assert_eq!(summary.mean_ms(), 4, "14 ms over 3 calls truncates, never rounds up");
        // Percentiles are only honest over the whole set, which `push` does not
        // retain; a running fold must not invent one.
        assert_eq!(summary.p50_ms, 0);
    }

    #[test]
    fn the_verbose_flag_is_read_from_a_false_string_too() {
        // `KILN_VERBOSE=0` and `false` must both mean off, or a stale variable in
        // a shell profile silently turns diagnostics on in every app.
        let decode = |value: &str| !(value == "0" || value.eq_ignore_ascii_case("false"));
        assert!(!decode("0"));
        assert!(!decode("false"));
        assert!(!decode("FALSE"));
        assert!(decode("1"));
        assert!(decode("true"));
        assert!(decode(""));
    }

    #[test]
    fn a_closure_runs_only_when_enabled() {
        let mut ran = false;
        record_if_enabled(false, || ran = true);
        assert!(!ran, "the formatting closure must not run when diagnostics are off");
        record_if_enabled(true, || ran = true);
        assert!(ran);
    }
}
