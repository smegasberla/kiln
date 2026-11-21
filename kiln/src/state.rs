//! Shell-owned state that outlives a hot reload.
//!
//! `kiln dev` rebuilds the commands cdylib and swaps it in. Anything the reloaded
//! code kept in a `static` is rebuilt with it, so a naive app silently loses its
//! counter, its open document, or its accumulated index on every save. The
//! honest fix is not to hide that, it is to give the shell a place to hold the
//! state instead, and to say so in the docs.
//!
//! ```ignore
//! #[cfg(feature = "hot-reload")]
//! fn bump() -> u64 {
//!     let n = kiln::state::get("count").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
//!     kiln::state::set("count", kiln::serde_json::json!(n));
//!     n
//! }
//! ```
//!
//! The store is a plain in-process map owned by the shell binary, so it survives
//! every generation of the library and dies with the process. It is compiled
//! unconditionally: it has no dependencies, a release build pays a few hundred
//! bytes, and an app can use it for scratch state without enabling hot reload.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

fn map() -> &'static Mutex<HashMap<String, Value>> {
    static MAP: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn generation() -> &'static AtomicU64 {
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    &GENERATION
}

/// Read a value written before the last reload, if it is still there.
///
/// A panic while holding the lock cannot happen here (no user code runs inside
/// it), but a poisoned lock is recovered rather than propagated: losing state is
/// better than taking the app down for it.
pub fn get(key: &str) -> Option<Value> {
    let map = map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.get(key).cloned()
}

/// Store a value that must outlive a reload. Replaces any previous value.
pub fn set(key: &str, value: Value) {
    let mut map = map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.insert(key.to_owned(), value);
}

/// Remove a value and return it.
pub fn remove(key: &str) -> Option<Value> {
    let mut map = map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.remove(key)
}

/// Whether `key` is currently set.
pub fn contains(key: &str) -> bool {
    let map = map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.contains_key(key)
}

/// Every stored key, sorted, so a reload report is stable and a test can assert
/// on it without depending on hash order.
pub fn keys() -> Vec<String> {
    let map = map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort_unstable();
    keys
}

/// Drop everything. The shell calls this only in its own tests, so a test cannot
/// be affected by state another test left behind.
pub fn clear() {
    let mut map = map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.clear();
}

/// Which generation of the commands library is live.
///
/// Starts at 0 for a released build and for the first dev load; each successful
/// hot swap increments it. Reloaded code can record what it did per generation
/// without the shell telling it, and a test can prove a value survived one.
pub fn reload_generation() -> u64 {
    generation().load(Ordering::SeqCst)
}

/// Record that a new library generation took over. Called by the hot loader
/// itself; a release build never reaches it, so it is not compiled there.
#[cfg(feature = "hot-reload")]
pub(crate) fn note_reload() -> u64 {
    generation().fetch_add(1, Ordering::SeqCst) + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The store is process-global by design, so two tests that write to it at
    /// the same time would trample each other's keys. Cargo runs tests in
    /// parallel threads, so each test that touches the store takes this first.
    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        clear();
        guard
    }

    #[test]
    fn values_round_trip_and_survive_being_read_twice() {
        let _guard = exclusive();
        assert_eq!(get("missing"), None);
        assert!(!contains("missing"));

        set("count", json!(3));
        assert_eq!(get("count"), Some(json!(3)));
        // Reading does not consume.
        assert_eq!(get("count"), Some(json!(3)));
        assert!(contains("count"));

        set("count", json!(4));
        assert_eq!(get("count"), Some(json!(4)));

        assert_eq!(remove("count"), Some(json!(4)));
        assert_eq!(remove("count"), None);
        assert!(!contains("count"));
        clear();
    }

    #[test]
    fn keys_are_sorted_and_reflect_removals() {
        let _guard = exclusive();
        set("b", json!(1));
        set("a", json!(2));
        set("c", json!(3));
        assert_eq!(keys(), vec!["a", "b", "c"]);
        remove("b");
        assert_eq!(keys(), vec!["a", "c"]);
        clear();
    }

    #[cfg(feature = "hot-reload")]
    #[test]
    fn a_reload_keeps_the_values_and_bumps_the_generation() {
        let _guard = exclusive();
        set("document", json!({ "title": "notes", "dirty": true }));
        let before = reload_generation();

        // A reload replaces the library, not the shell: the map is untouched and
        // only the counter moves.
        let after = note_reload();
        assert_eq!(after, before + 1);
        assert_eq!(
            get("document"),
            Some(json!({ "title": "notes", "dirty": true })),
            "state written before a reload must be readable after it"
        );
        clear();
    }

    #[test]
    fn concurrent_writers_do_not_lose_entries() {
        let _guard = exclusive();
        std::thread::scope(|scope| {
            for n in 0..16 {
                scope.spawn(move || {
                    set(&format!("k{n}"), json!(n));
                });
            }
        });
        assert_eq!(keys().len(), 16);
        clear();
    }
}
