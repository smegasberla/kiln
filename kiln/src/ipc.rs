//! The command registry and the wire envelope.
//!
//! One endpoint (`POST kiln://localhost/__kiln/<method>`), one envelope shape
//! in both directions:
//!
//! ```text
//! request  body: <payload as JSON, or null>
//! response body: {"ok": <json>} | {"error": {"message": "<text>", "kind": "<class>"}}
//! ```
//!
//! The whitelist is the set of handlers compiled into the binary. There is no
//! capability file, no ACL directory, and no runtime permission surface: if a
//! command is not registered in Rust, no page can call it.
//!
//! Handlers come in two kinds. Sync handlers run inline on the UI thread, which
//! is right for returning a value you already hold. Async handlers run on a
//! worker thread and reply when they finish, so a slow filesystem write or a
//! network call cannot freeze the window.

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};

use serde_json::Value;

use crate::{AppHandle, Error};

/// The path prefix the shell reserves for the invoke endpoint.
pub const ENDPOINT_PREFIX: &str = "/__kiln/";

type SyncHandler = Box<dyn Fn(&AppHandle, Value) -> Result<Value, Error> + Send + Sync>;
/// Async handlers are shared rather than borrowed so a worker thread can own a
/// reference to one after `dispatch_deferred` returns.
type AsyncHandler = std::sync::Arc<dyn Fn(AppHandle, Value) -> Result<Value, Error> + Send + Sync>;

/// Deferred handlers own their completion callback, so they can answer after
/// arbitrary work and from any thread or task.
#[cfg(feature = "rt")]
type DeferredHandler = Box<dyn Fn(AppHandle, Value, Box<dyn FnOnce(String) + Send>) + Send + Sync>;

enum Handler {
    Sync(SyncHandler),
    Async(AsyncHandler),
    /// Owns its completion callback, so it can answer after arbitrary work and
    /// from any thread or task. The async runtime registers here; nothing about
    /// the envelope changes.
    #[cfg(feature = "rt")]
    Deferred(DeferredHandler),
}

/// Name -> handler table plus the optional config allowlist.
pub struct Registry {
    handlers: HashMap<String, Handler>,
    allow: Option<Vec<String>>,
    /// Per-window allowlists keyed by window label, from `kiln.json`'s
    /// `windows` map or from `WindowOptions::allowlist`. A listed window's
    /// surface is the intersection of its list with the global allowlist, so a
    /// config entry can only narrow.
    ///
    /// Shared behind a lock because a window opened *at runtime* registers its
    /// own narrowing after the registry was already moved into the `Arc` the
    /// handlers hold. Without that, a window created from a command would only
    /// be narrowable by pre-declaring its label in a config file.
    window_allow: std::sync::Arc<std::sync::RwLock<HashMap<String, Vec<String>>>>,
    /// The hot-reloaded commands library. Present only in `kiln dev` builds;
    /// a release build never compiles it.
    #[cfg(feature = "hot-reload")]
    hot: Option<std::sync::Arc<crate::hot::HotLib>>,
    /// Dispatch latency samples per method, filled only while diagnostics are
    /// on. Behind an `Arc` so an async dispatch can record from its worker
    /// thread without borrowing the registry.
    latency: std::sync::Arc<std::sync::Mutex<HashMap<String, Vec<u64>>>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            allow: None,
            latency: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            window_allow: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
            #[cfg(feature = "hot-reload")]
            hot: None,
        }
    }

    /// Attach the hot-reloaded commands library (`kiln dev`). Static handlers
    /// keep priority; the library only ever answers names no static handler
    /// claimed, and only within the config allowlist like everything else.
    #[cfg(feature = "hot-reload")]
    pub(crate) fn set_hot(&mut self, hot: std::sync::Arc<crate::hot::HotLib>) {
        self.hot = Some(hot);
    }

    pub fn set_allowlist(&mut self, allow: Option<Vec<String>>) {
        self.allow = allow;
    }

    /// Attach per-window allowlists keyed by window label.
    ///
    /// Additive: a label declared in `kiln.json` keeps its entry, and a runtime
    /// window adds or replaces only its own.
    pub fn set_window_allowlist(&mut self, allow: HashMap<String, Vec<String>>) {
        let mut current = self
            .window_allow
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (label, list) in allow {
            current.insert(label, list);
        }
    }

    /// Narrow one window's surface at runtime.
    ///
    /// Called when a window with `WindowOptions::allowlist` is created, so the
    /// restriction exists before the page inside it can call anything. It is the
    /// same intersection as a config entry: a runtime list can only ever remove
    /// commands, never add one the app withheld.
    pub fn allow_window(&self, label: &str, commands: Vec<String>) {
        let mut allow = self
            .window_allow
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        allow.insert(label.to_owned(), commands);
    }

    /// The window labels with a narrowing entry, for diagnostics and tests.
    pub fn narrowed_windows(&self) -> Vec<String> {
        let mut labels: Vec<String> = self
            .window_allow
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .cloned()
            .collect();
        labels.sort_unstable();
        labels
    }

    /// Whether `method` is registered and allowed, globally or for `label`.
    ///
    /// A window with a config entry can only call what is in *both* the global
    /// allowlist and its own list, so a per-window entry can never widen what
    /// the app already allows.
    fn permitted_for(&self, method: &str, label: Option<&str>) -> bool {
        if !self.permitted(method) {
            return false;
        }
        let label = match label {
            Some(label) => label,
            None => return true,
        };
        let allow = self
            .window_allow
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match allow.get(label) {
            Some(list) => list.iter().any(|allowed| allowed == method),
            None => true,
        }
    }

    /// Register a handler that runs on the UI thread. Registering the same name
    /// twice replaces it.
    pub fn add<F>(&mut self, method: &str, handler: F)
    where
        F: Fn(&AppHandle, Value) -> Result<Value, Error> + Send + Sync + 'static,
    {
        self.handlers
            .insert(method.to_owned(), Handler::Sync(Box::new(handler)));
    }

    /// Register a handler that runs on a worker thread.
    ///
    /// Use this for anything touching the disk, the network, or that can take
    /// longer than a frame: the reply arrives when the work is done and the
    /// window keeps painting meanwhile.
    pub fn add_async<F>(&mut self, method: &str, handler: F)
    where
        F: Fn(AppHandle, Value) -> Result<Value, Error> + Send + Sync + 'static,
    {
        self.handlers.insert(
            method.to_owned(),
            Handler::Async(std::sync::Arc::new(handler)),
        );
    }

    /// Register a command whose argument and reply are concrete types.
    ///
    /// The payload is deserialized before the handler runs, so a wrong shape
    /// becomes an [`Error::Payload`] the page can branch on instead of a
    /// handler string-match. The reply is serialized back into the same
    /// envelope, so the wire format does not change.
    pub fn add_typed<P, R, F>(&mut self, method: &str, handler: F)
    where
        P: crate::serde::de::DeserializeOwned + Send + 'static,
        R: crate::serde::Serialize + 'static,
        F: Fn(&AppHandle, P) -> Result<R, Error> + Send + Sync + 'static,
    {
        let handler = std::sync::Arc::new(handler);
        self.add(method, move |handle, payload| {
            let args: P = decode_args(payload)?;
            let reply = handler(handle, args)?;
            serde_json::to_value(reply).map_err(|error| Error::Handler(error.to_string()))
        });
    }

    /// Register a real `async fn` command. Requires the `rt` feature.
    ///
    /// The payload is deserialized on the calling thread, so a malformed
    /// argument is answered immediately; the future itself runs on the shared
    /// runtime, where a thousand in-flight requests cost a thousand cheap tasks
    /// rather than a thousand stacks. The reply goes through the same envelope
    /// as every other handler.
    ///
    /// A panic inside the future is caught and reported as `kind: "panic"`
    /// instead of killing the worker task, which would otherwise leave the page
    /// awaiting a reply forever.
    #[cfg(feature = "rt")]
    pub fn add_async_fn<P, R, F, Fut>(&mut self, method: &str, handler: F)
    where
        P: crate::serde::de::DeserializeOwned + Send + 'static,
        R: crate::serde::Serialize + 'static,
        F: Fn(AppHandle, P) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<R, Error>> + Send + 'static,
    {
        let handler = std::sync::Arc::new(handler);
        let method = method.to_owned();
        self.handlers.insert(
            method.clone(),
            Handler::Deferred(Box::new(move |handle, payload, done| {
                let args = match decode_args::<P>(payload) {
                    Ok(args) => args,
                    Err(error) => return done(ok_or_error(&method, Err(error))),
                };
                let handler = std::sync::Arc::clone(&handler);
                let method = method.clone();
                crate::rt::spawn(async move {
                    let result = match crate::rt::isolate(handler(handle, args)).await {
                        Ok(result) => result,
                        Err(message) => Err(Error::Panic(message)),
                    };
                    let result = result.and_then(|reply| {
                        serde_json::to_value(reply)
                            .map_err(|error| Error::Handler(error.to_string()))
                    });
                    done(ok_or_error(&method, result));
                });
            })),
        );
    }

    /// Names that can actually be called, sorted, for diagnostics.
    pub fn callable(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .handlers
            .keys()
            .filter(|name| self.permitted(name))
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names
    }

    /// Whether `method` is registered and allowed.
    pub fn has(&self, method: &str) -> bool {
        self.permitted(method) && self.handlers.contains_key(method)
    }

    fn permitted(&self, method: &str) -> bool {
        match &self.allow {
            Some(allow) => allow.iter().any(|a| a == method),
            None => true,
        }
    }

    /// Run one request and produce the response body.
    ///
    /// Errors are converted into the envelope rather than propagated, so the
    /// page always gets valid JSON it can act on, and a handler failure never
    /// takes down the window.
    pub fn dispatch(&self, handle: &AppHandle, method: &str, payload: Value) -> String {
        let (tx, rx) = std::sync::mpsc::channel();
        self.dispatch_deferred(handle, method, payload, move |body| {
            let _ = tx.send(body);
        });
        rx.recv()
            .unwrap_or_else(|_| error_envelope(&Error::Handler("no response".to_owned())))
    }

    /// Run one request, passing the response body to `done`.
    ///
    /// Sync handlers call `done` before returning; async handlers call it from
    /// the worker thread that runs them, which is what lets the protocol layer
    /// answer after the window has already moved on.
    pub fn dispatch_deferred<F>(&self, handle: &AppHandle, method: &str, payload: Value, done: F)
    where
        F: FnOnce(String) + Send + 'static,
    {
        self.dispatch_gated(handle, None, method, payload, done)
    }

    /// Dispatch a request that claims to come from the window `label`.
    ///
    /// The page cannot be trusted to name its window, so `label` comes from the
    /// webview the request arrived on, not from the payload. `None` means the
    /// request is app-level (a menu item, an Android bridge fallback) and gets
    /// the global allowlist.
    pub fn dispatch_for_window<F>(
        &self,
        handle: &AppHandle,
        label: Option<&str>,
        method: &str,
        payload: Value,
        done: F,
    ) where
        F: FnOnce(String) + Send + 'static,
    {
        self.dispatch_gated(handle, label, method, payload, done)
    }

    fn dispatch_gated<F>(
        &self,
        handle: &AppHandle,
        label: Option<&str>,
        method: &str,
        payload: Value,
        done: F,
    ) where
        F: FnOnce(String) + Send + 'static,
    {
        // The allowlists gate everything below, the hot library included:
        // `commands` and per-window lists in kiln.json can only ever narrow
        // what is reachable.
        if !self.permitted_for(method, label) {
            return done(error_envelope_for(
                method,
                &Error::UnknownMethod(method.to_owned()),
            ));
        }
        // Timed around the handler, not the whole transport: this is the number
        // `kiln dev --verbose` reports as command latency. Reading a `bool` is
        // free and `Instant::now` is skipped entirely when diagnostics are off.
        let timed = crate::diagnostics::enabled();

        match self.handlers.get(method) {
            Some(Handler::Sync(handler)) => {
                let started = timed.then(std::time::Instant::now);
                let outcome = run_isolated(handler, handle, payload);
                if let Some(started) = started {
                    self.note_latency(method, started.elapsed());
                }
                done(ok_or_error(method, outcome))
            }
            Some(Handler::Async(handler)) => {
                let handler = std::sync::Arc::clone(handler);
                let handle = handle.clone();
                let method = method.to_owned();
                let samples = std::sync::Arc::clone(&self.latency);
                std::thread::spawn(move || {
                    let started = timed.then(std::time::Instant::now);
                    let outcome = run_isolated_async(&handler, handle, payload);
                    if let Some(started) = started {
                        record_latency(&samples, &method, started.elapsed());
                    }
                    done(ok_or_error(&method, outcome));
                });
            }
            #[cfg(feature = "rt")]
            Some(Handler::Deferred(handler)) => {
                handler(handle.clone(), payload, Box::new(done));
            }
            None => self.dispatch_unclaimed(method, payload, done),
        }
    }

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
    /// This is what `kiln dev --verbose` prints when the app exits: the map is
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

    fn note_latency(&self, method: &str, elapsed: std::time::Duration) {
        record_latency(&self.latency, method, elapsed);
    }

    /// A name no static handler claimed. Release: unknown command, exactly as
    /// before. `kiln dev` with an attached library: a call into the current
    /// commands dylib, answered from a worker thread so a slow hot command
    /// cannot freeze the window either.
    #[cfg(feature = "hot-reload")]
    fn dispatch_unclaimed<F>(&self, method: &str, payload: Value, done: F)
    where
        F: FnOnce(String) + Send + 'static,
    {
        match &self.hot {
            Some(hot) => {
                let hot = std::sync::Arc::clone(hot);
                let method = method.to_owned();
                std::thread::spawn(move || {
                    // Belt over the dylib's own catch_unwind: the page gets a
                    // response even if this outer layer is what failed.
                    let body =
                        panic::catch_unwind(AssertUnwindSafe(|| hot.call(&method, &payload)))
                            .unwrap_or_else(|_| {
                                error_envelope(&Error::Handler(
                                    "hot command call failed".to_owned(),
                                ))
                            });
                    done(body);
                });
            }
            None => done(error_envelope_for(
                method,
                &Error::UnknownMethod(method.to_owned()),
            )),
        }
    }

    #[cfg(not(feature = "hot-reload"))]
    fn dispatch_unclaimed<F>(&self, method: &str, _payload: Value, done: F)
    where
        F: FnOnce(String) + Send + 'static,
    {
        done(error_envelope_for(
            method,
            &Error::UnknownMethod(method.to_owned()),
        ))
    }
}

/// Run one handler such that a panic in it cannot take down the app.
///
/// Handlers are called from inside wry's FFI protocol callback, where unwinding
/// across the C boundary is undefined behavior, so a panic there aborts the
/// process. Catching here means an app with a bug in one command reports the
/// error to the page and keeps the window (and any unsaved work) alive.
///
/// `AssertUnwindSafe` is sound here because the handler is the only thing being
/// unwound: registry state is read-only, and a handler that panicked mid-way
/// leaves its own data as it was, which is what the page is told about.
fn run_isolated(handler: &SyncHandler, handle: &AppHandle, payload: Value) -> Result<Value, Error> {
    match panic::catch_unwind(AssertUnwindSafe(|| handler(handle, payload))) {
        Ok(result) => result,
        Err(payload) => Err(report_panic(payload.as_ref())),
    }
}

fn run_isolated_async(
    handler: &AsyncHandler,
    handle: AppHandle,
    payload: Value,
) -> Result<Value, Error> {
    match panic::catch_unwind(AssertUnwindSafe(|| handler(handle, payload))) {
        Ok(result) => result,
        Err(payload) => Err(report_panic(payload.as_ref())),
    }
}

/// Deserialize a handler's arguments, treating an absent body as `{}`.
///
/// A page that calls `invoke("open_settings")` sends no body at all, and a
/// struct type cannot be built from `null`. The natural reading is "no fields
/// given", so a null payload falls back to an empty object before failing.
/// `()` and `Value` are unaffected: they accept `null` directly and are tried
/// first, so their meaning never shifts.
fn decode_args<P>(payload: Value) -> Result<P, Error>
where
    P: crate::serde::de::DeserializeOwned,
{
    let absent = payload.is_null();
    match serde_json::from_value(payload) {
        Ok(args) => Ok(args),
        // A body-less call is "no fields given", which is an empty object for a
        // struct type. Only a null body is retried, so a present-but-wrong field
        // is still a rejection rather than a silent default. `()` and `Value`
        // accept null directly and never reach here.
        Err(error) if absent => serde_json::from_value(Value::Object(serde_json::Map::new()))
            .map_err(|_| Error::Payload(error.to_string())),
        Err(error) => Err(Error::Payload(error.to_string())),
    }
}

fn ok_or_error(method: &str, result: Result<Value, Error>) -> String {
    match result {
        Ok(value) => format!(r#"{{"ok":{value}}}"#),
        Err(error) => error_envelope_for(method, &error),
    }
}

/// Build the error side of the envelope: a message plus a machine-readable
/// `kind` so a page branches on the failure class instead of matching text.
/// `unknown` deliberately covers both an unregistered command and an allowlist
/// denial, so the page cannot tell which commands exist but were withheld.
pub(crate) fn error_envelope(error: &Error) -> String {
    error_envelope_for("", error)
}

/// The failure envelope for a known method, so the page can tell *which*
/// command failed without parsing the message text.
///
/// `method` is the name the page asked for. An empty one means the answer came
/// from a path that has no request behind it (a hot-library boundary failure),
/// and the field is still present so the shape never varies.
pub(crate) fn error_envelope_for(method: &str, error: &Error) -> String {
    let message = serde_json::to_string(&error.to_string())
        .unwrap_or_else(|_| "\"command failed\"".to_owned());
    let kind = serde_json::Value::from(error.kind());
    let method = serde_json::Value::from(method);
    format!(r#"{{"error":{{"message":{message},"kind":{kind},"method":{method}}}}}"#)
}

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
fn record_latency(
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

fn report_panic(payload: &(dyn std::any::Any + Send)) -> Error {
    let message = panic_message(payload);
    crate::diagnostics::record(
        crate::diagnostics::Kind::App,
        "handler panicked",
        serde_json::json!({ "message": message }),
    );
    Error::Panic(message)
}

/// Best-effort text out of a `panic!` payload, usually `&str`. Shared with
/// the hot-reload path, which reports a boundary panic the same way.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "unknown panic".to_owned()
    }
}

/// Extract the method name from an endpoint path, decoding `%XX` escapes.
///
/// Returns `None` for anything outside the endpoint namespace so the caller can
/// fall through to static asset serving.
pub fn method_from_path(path: &str) -> Option<String> {
    let raw = path.strip_prefix(ENDPOINT_PREFIX)?;
    if raw.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &raw[i + 1..i + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::mpsc;

    fn registry_with(names: &[&str]) -> Registry {
        let mut reg = Registry::new();
        for name in names {
            let owned = (*name).to_owned();
            reg.add(name, move |_h, payload| {
                Ok(json!({ "echo": payload, "name": owned }))
            });
        }
        reg
    }

    fn empty_handle() -> AppHandle {
        AppHandle::detached_for_tests()
    }

    #[test]
    fn happy_path_wraps_in_ok() {
        let reg = registry_with(&["greet"]);
        let out = reg.dispatch(&empty_handle(), "greet", json!({ "a": 1 }));
        assert_eq!(out, r#"{"ok":{"echo":{"a":1},"name":"greet"}}"#);
    }

    #[test]
    fn unknown_method_is_an_error_envelope_not_a_panic() {
        let reg = registry_with(&["greet"]);
        let out = reg.dispatch(&empty_handle(), "nope", Value::Null);
        assert!(
            out.contains("unknown command: nope") && out.contains(r#""kind":"unknown""#),
            "{out}"
        );
    }

    #[test]
    fn allowlist_restricts_registered_commands() {
        let mut reg = registry_with(&["a", "b"]);
        reg.set_allowlist(Some(vec!["a".to_owned()]));
        assert_eq!(reg.callable(), vec!["a"]);
        assert!(reg
            .dispatch(&empty_handle(), "b", Value::Null)
            .contains("error"));
        assert!(reg
            .dispatch(&empty_handle(), "a", Value::Null)
            .contains(r#""ok""#));
    }

    fn dispatch_for_payload(
        reg: &Registry,
        label: Option<&str>,
        method: &str,
        payload: Value,
    ) -> String {
        let (tx, rx) = mpsc::channel();
        reg.dispatch_for_window(&empty_handle(), label, method, payload, move |body| {
            let _ = tx.send(body);
        });
        rx.recv().unwrap()
    }

    fn dispatch_for(reg: &Registry, label: Option<&str>, method: &str) -> String {
        let (tx, rx) = mpsc::channel();
        reg.dispatch_for_window(&empty_handle(), label, method, Value::Null, move |body| {
            let _ = tx.send(body);
        });
        rx.recv().unwrap()
    }

    #[test]
    fn a_window_entry_narrows_to_a_strict_subset() {
        let mut reg = registry_with(&["a", "b"]);
        reg.set_window_allowlist(HashMap::from([(
            "preview".to_owned(),
            vec!["a".to_owned()],
        )]));

        // The preview window can only call "a"; the denial looks like an
        // unknown command, so the page cannot tell which commands exist.
        assert!(dispatch_for(&reg, Some("preview"), "a").contains(r#""ok""#));
        assert!(dispatch_for(&reg, Some("preview"), "b").contains(r#""kind":"unknown""#));

        // A window without a config entry keeps the global surface.
        assert!(dispatch_for(&reg, Some("main"), "b").contains(r#""ok""#));

        // App-level dispatch (menus, the Android fallback) is untouched.
        assert!(reg
            .dispatch(&empty_handle(), "b", Value::Null)
            .contains(r#""ok""#));
    }

    #[test]
    fn a_window_entry_cannot_widen_the_global_allowlist() {
        let mut reg = registry_with(&["a", "b"]);
        reg.set_allowlist(Some(vec!["a".to_owned()]));
        reg.set_window_allowlist(HashMap::from([(
            "preview".to_owned(),
            vec!["a".to_owned(), "b".to_owned()],
        )]));
        assert!(dispatch_for(&reg, Some("preview"), "a").contains(r#""ok""#));
        assert!(dispatch_for(&reg, Some("preview"), "b").contains(r#""kind":"unknown""#));
    }

    #[test]
    fn dispatch_latency_is_recorded_only_while_diagnostics_are_on() {
        let _guard = crate::diagnostics::test_guard();
        // One test, both states: a release build must not pay for the map, and a
        // verbose dev run must actually get numbers out of it.
        let reg = registry_with(&["greet"]);
        let handle = crate::AppHandle::detached_for_tests();

        crate::diagnostics::set_enabled(false);
        assert_eq!(reg.clear_latencies(), 0);
        let _ = reg.dispatch(&handle, "greet", Value::Null);
        assert!(
            reg.latencies().is_empty(),
            "a timed dispatch must cost nothing when diagnostics are off"
        );

        crate::diagnostics::set_enabled(true);
        let _ = reg.dispatch(&handle, "greet", Value::Null);
        let _ = reg.dispatch(&handle, "greet", Value::Null);
        let latencies = reg.latencies();
        crate::diagnostics::set_enabled(false);

        let (method, summary) = latencies.first().expect("a timed command is reported");
        assert_eq!(method, "greet");
        assert_eq!(summary.count, 2, "both calls are counted");
        assert_eq!(reg.clear_latencies(), 2);
        assert!(reg.latencies().is_empty());
    }

    #[test]
    fn a_latency_report_is_sorted_slowest_first_and_names_each_command() {
        let _guard = crate::diagnostics::test_guard();
        let reg = registry_with(&["fast", "slow"]);
        let handle = crate::AppHandle::detached_for_tests();
        crate::diagnostics::set_enabled(true);
        let _ = reg.dispatch(&handle, "fast", Value::Null);
        let _ = reg.dispatch(&handle, "slow", Value::Null);
        let report = reg.latency_report();
        crate::diagnostics::set_enabled(false);

        let commands = report.as_array().expect("a report is an array");
        assert_eq!(commands.len(), 2);
        for command in commands {
            assert!(command["method"].is_string());
            assert!(command["p50_ms"].is_number());
            assert!(command["count"].as_u64().unwrap_or(0) >= 1);
        }
        // Sorted by mean, so a reader takes the first line as the one to fix.
        let means: Vec<u64> = commands
            .iter()
            .map(|command| command["mean_ms"].as_u64().unwrap_or(0))
            .collect();
        assert!(means.windows(2).all(|pair| pair[0] >= pair[1]), "{means:?}");
    }

    #[test]
    fn a_runtime_narrowing_applies_to_the_window_it_names() {
        let reg = registry_with(&["greet", "save"]);
        // Registered after construction, which is what a window opened from a
        // command does: the registry is already shared by its handlers.
        reg.allow_window("preview", vec!["greet".to_owned()]);

        assert!(dispatch_for(&reg, Some("preview"), "greet").contains(r#""ok""#));
        assert!(
            dispatch_for(&reg, Some("preview"), "save").contains(r#""kind":"unknown""#),
            "a runtime lane must withhold what it does not name"
        );
        // Every other window, and app-level requests, keep the global surface.
        assert!(dispatch_for(&reg, None, "save").contains(r#""ok""#));
        assert!(dispatch_for(&reg, Some("main"), "save").contains(r#""ok""#));
    }

    #[test]
    fn a_runtime_narrowing_cannot_widen_the_global_allowlist() {
        let mut reg = registry_with(&["a", "b"]);
        reg.set_allowlist(Some(vec!["a".to_owned()]));
        reg.allow_window("preview", vec!["a".to_owned(), "b".to_owned()]);

        assert!(dispatch_for(&reg, Some("preview"), "a").contains(r#""ok""#));
        assert!(dispatch_for(&reg, Some("preview"), "b").contains(r#""kind":"unknown""#));
    }

    #[test]
    fn config_and_runtime_narrowings_accumulate_rather_than_replace() {
        let mut reg = registry_with(&["a", "b"]);
        reg.set_window_allowlist(HashMap::from([(
            "from-config".to_owned(),
            vec!["a".to_owned()],
        )]));
        reg.allow_window("from-runtime", vec!["b".to_owned()]);

        assert_eq!(
            reg.narrowed_windows(),
            vec!["from-config".to_owned(), "from-runtime".to_owned()]
        );
        assert!(dispatch_for(&reg, Some("from-config"), "a").contains(r#""ok""#));
        assert!(dispatch_for(&reg, Some("from-runtime"), "b").contains(r#""ok""#));
    }

    #[test]
    fn an_unlabelled_window_request_uses_the_global_allowlist() {
        let mut reg = registry_with(&["a", "b"]);
        reg.set_allowlist(Some(vec!["a".to_owned()]));
        assert!(dispatch_for(&reg, None, "a").contains(r#""ok""#));
        assert!(dispatch_for(&reg, None, "b").contains(r#""kind":"unknown""#));
    }

    #[test]
    fn a_bodyless_typed_command_reads_as_an_empty_argument_struct() {
        #[derive(crate::serde::Deserialize, Default)]
        #[serde(default, crate = "crate::serde")]
        struct Args {
            title: Option<String>,
        }

        let mut reg = Registry::new();
        reg.add_typed("open_settings", |_h, args: Args| {
            Ok(json!({ "title": args.title }))
        });

        // No body at all: the page called `invoke("open_settings")`.
        assert_eq!(
            reg.dispatch(&empty_handle(), "open_settings", Value::Null),
            r#"{"ok":{"title":null}}"#
        );
        // An explicit empty object behaves the same.
        assert_eq!(
            reg.dispatch(&empty_handle(), "open_settings", json!({})),
            r#"{"ok":{"title":null}}"#
        );
        // A wrong *field type* is still a rejection, not a silent default.
        let parsed: Value = serde_json::from_str(&reg.dispatch(
            &empty_handle(),
            "open_settings",
            json!({ "title": 5 }),
        ))
        .unwrap();
        assert_eq!(parsed["error"]["kind"], "payload");
    }

    #[test]
    fn a_bodyless_call_still_works_for_unit_and_value_arguments() {
        let mut reg = Registry::new();
        reg.add_typed("unit", |_h, _args: ()| Ok(json!("unit")));
        reg.add_typed("raw", |_h, args: Value| Ok(args));

        assert_eq!(
            reg.dispatch(&empty_handle(), "unit", Value::Null),
            r#"{"ok":"unit"}"#
        );
        // `Value` keeps receiving the real payload rather than being rewritten.
        assert_eq!(
            reg.dispatch(&empty_handle(), "raw", json!({ "a": 1 })),
            r#"{"ok":{"a":1}}"#
        );
        assert_eq!(
            reg.dispatch(&empty_handle(), "raw", Value::Null),
            r#"{"ok":null}"#
        );
    }

    #[test]
    fn a_typed_command_round_trips_through_the_envelope() {
        #[derive(crate::serde::Deserialize, crate::serde::Serialize)]
        struct Args {
            name: String,
        }

        let mut reg = Registry::new();
        reg.add_typed("greet", |_h, args: Args| Ok(json!({ "hello": args.name })));

        let out = reg.dispatch(&empty_handle(), "greet", json!({ "name": "Lin" }));
        assert_eq!(out, r#"{"ok":{"hello":"Lin"}}"#);
    }

    #[test]
    fn a_typed_command_rejects_a_wrong_shape_as_a_payload_error() {
        #[derive(crate::serde::Deserialize)]
        struct Args {
            #[allow(dead_code)]
            name: String,
        }

        let mut reg = Registry::new();
        reg.add_typed("greet", |_h, _args: Args| Ok(json!("hi")));

        let out = reg.dispatch(&empty_handle(), "greet", json!({ "nope": 1 }));
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["error"]["kind"], "payload", "{out}");
        assert_eq!(parsed["error"]["method"], "greet", "{out}");
    }

    #[test]
    fn a_typed_command_still_respects_the_allowlist() {
        #[derive(crate::serde::Deserialize, crate::serde::Serialize)]
        struct Args {
            #[allow(dead_code)]
            name: String,
        }

        let mut reg = Registry::new();
        reg.add_typed("greet", |_h, args: Args| Ok(args));
        reg.set_allowlist(Some(vec!["other".to_owned()]));

        let out = reg.dispatch(&empty_handle(), "greet", json!({ "name": "Lin" }));
        assert!(out.contains(r#""kind":"unknown""#), "{out}");
    }

    #[test]
    fn a_typed_command_can_still_fail_like_any_other() {
        #[derive(crate::serde::Deserialize)]
        struct Args {
            #[allow(dead_code)]
            name: String,
        }

        let mut reg = Registry::new();
        reg.add_typed("greet", |_h, _args: Args| {
            Err::<Value, _>(Error::Handler("nope".to_owned()))
        });

        let parsed: Value =
            serde_json::from_str(&reg.dispatch(&empty_handle(), "greet", json!({ "name": "L" })))
                .unwrap();
        assert_eq!(parsed["error"]["kind"], "handler");
        assert_eq!(parsed["error"]["message"], "nope");
    }

    #[test]
    fn every_failure_kind_carries_its_method_and_a_distinct_kind() {
        let mut reg = Registry::new();
        reg.add("handler", |_h, _p| Err(Error::Handler("bad".to_owned())));
        reg.add("boom", |_h, _p| panic!("exploded"));
        reg.add_typed("typed", |_h, _p: String| Ok(_p));
        reg.add("runtime", |_h, _p| {
            Err::<Value, _>(Error::Runtime("loop".to_owned()))
        });
        reg.set_window_allowlist(HashMap::from([("preview".to_owned(), vec![])]));

        let cases: [(&str, &str, Option<&str>); 6] = [
            ("handler", "handler", None),
            ("boom", "panic", None),
            ("typed", "payload", None),
            ("runtime", "runtime", None),
            ("missing", "unknown", None),
            ("handler", "unknown", Some("preview")),
        ];
        for (method, expected, label) in cases {
            let body = match label {
                Some(label) => dispatch_for_payload(&reg, Some(label), method, json!(1)),
                None => dispatch_for_payload(&reg, None, method, json!(1)),
            };
            let parsed: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(parsed["error"]["kind"], expected, "{method}: {body}");
            assert_eq!(parsed["error"]["method"], method, "{method}: {body}");
            assert!(parsed["error"]["message"].is_string(), "{method}: {body}");
        }
    }

    #[cfg(feature = "rt")]
    #[test]
    fn an_async_fn_runs_off_the_caller_thread_and_answers() {
        #[derive(crate::serde::Deserialize)]
        struct Args {
            ms: u64,
        }

        let caller = std::thread::current().id();
        let mut reg = Registry::new();
        reg.add_async_fn("wait", move |_h, args: Args| async move {
            tokio::time::sleep(std::time::Duration::from_millis(args.ms)).await;
            Ok(json!({ "off_caller_thread": std::thread::current().id() != caller }))
        });

        let (tx, rx) = mpsc::channel();
        reg.dispatch_deferred(&empty_handle(), "wait", json!({ "ms": 1 }), move |body| {
            let _ = tx.send(body);
        });
        let body = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(body, r#"{"ok":{"off_caller_thread":true}}"#);
    }

    #[cfg(feature = "rt")]
    #[test]
    fn an_async_fn_rejects_a_bad_payload_without_touching_the_runtime() {
        #[derive(crate::serde::Deserialize)]
        struct Args {
            #[allow(dead_code)]
            ms: u64,
        }

        let mut reg = Registry::new();
        reg.add_async_fn("wait", |_h, args: Args| async move { Ok(json!(args.ms)) });

        let (tx, rx) = mpsc::channel();
        reg.dispatch_deferred(&empty_handle(), "wait", json!("nope"), move |body| {
            let _ = tx.send(body);
        });
        let body = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["error"]["kind"], "payload", "{body}");
        assert_eq!(parsed["error"]["method"], "wait");
    }

    #[cfg(feature = "rt")]
    #[test]
    fn a_panicking_async_fn_reports_a_panic_instead_of_hanging_the_page() {
        #[derive(crate::serde::Deserialize)]
        struct Args {
            #[allow(dead_code)]
            ms: u64,
        }

        let mut reg = Registry::new();
        reg.add_async_fn("boom", |_h, _args: Args| async move {
            panic!("async exploded");
            #[allow(unreachable_code)]
            Ok::<Value, Error>(Value::Null)
        });

        let (tx, rx) = mpsc::channel();
        reg.dispatch_deferred(&empty_handle(), "boom", json!({ "ms": 1 }), move |body| {
            let _ = tx.send(body);
        });
        let body = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["error"]["kind"], "panic", "{body}");
        assert!(
            parsed["error"]["message"]
                .as_str()
                .unwrap()
                .contains("async exploded"),
            "{body}"
        );
    }

    #[cfg(feature = "rt")]
    #[test]
    fn an_async_fn_respects_the_allowlist_like_every_other_command() {
        #[derive(crate::serde::Deserialize)]
        struct Args {
            #[allow(dead_code)]
            ms: u64,
        }

        let mut reg = Registry::new();
        reg.add_async_fn("wait", |_h, args: Args| async move { Ok(json!(args.ms)) });
        reg.set_allowlist(Some(vec!["other".to_owned()]));

        let (tx, rx) = mpsc::channel();
        reg.dispatch_deferred(&empty_handle(), "wait", json!({ "ms": 1 }), move |body| {
            let _ = tx.send(body);
        });
        let body = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert!(body.contains(r#""kind":"unknown""#), "{body}");
    }

    #[cfg(feature = "rt")]
    #[test]
    fn many_concurrent_async_fns_all_answer() {
        #[derive(crate::serde::Deserialize)]
        struct Args {
            n: u64,
        }

        let mut reg = std::sync::Arc::new(Registry::new());
        std::sync::Arc::get_mut(&mut reg).unwrap().add_async_fn(
            "double",
            |_h, args: Args| async move { Ok(json!(args.n * 2)) },
        );

        let (tx, rx) = mpsc::channel();
        for n in 0..64u64 {
            let reg = std::sync::Arc::clone(&reg);
            let tx = tx.clone();
            let handle = empty_handle();
            std::thread::spawn(move || {
                reg.dispatch_deferred(&handle, "double", json!({ "n": n }), move |body| {
                    let _ = tx.send(body);
                });
            });
        }
        drop(tx);

        let mut seen = std::collections::HashSet::new();
        for body in rx.iter().take(64) {
            let parsed: Value = serde_json::from_str(&body).unwrap();
            let value = parsed["ok"].as_u64().unwrap_or_else(|| panic!("{body}"));
            seen.insert(value / 2);
        }
        assert_eq!(seen.len(), 64, "every request must answer exactly once");
    }

    #[test]
    fn handler_errors_become_error_envelopes() {
        let mut reg = Registry::new();
        reg.add("boom", |_h, _p| Err(Error::Handler("kaboom".into())));
        let out = reg.dispatch(&empty_handle(), "boom", Value::Null);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["error"]["message"], "kaboom");
        assert_eq!(parsed["error"]["kind"], "handler");
    }

    #[test]
    fn a_panicking_handler_reports_an_error_instead_of_killing_the_app() {
        let mut reg = Registry::new();
        reg.add("boom", |_h, _p| {
            let empty: Vec<u8> = Vec::new();
            let _ = empty[3];
            Ok(Value::Null)
        });
        let out = reg.dispatch(&empty_handle(), "boom", Value::Null);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let message = parsed["error"]["message"]
            .as_str()
            .expect("error envelope expected");
        assert!(message.starts_with("command panicked:"), "{message}");
        assert!(message.contains("index out of bounds"), "{message}");
    }

    #[test]
    fn a_panicking_handler_does_not_poison_later_calls() {
        let mut reg = Registry::new();
        reg.add("boom", |_h, _p| panic!("kaboom"));
        reg.add("fine", |_h, _p| Ok(json!("still here")));

        assert!(reg
            .dispatch(&empty_handle(), "boom", Value::Null)
            .contains("error"));
        assert_eq!(
            reg.dispatch(&empty_handle(), "fine", Value::Null),
            r#"{"ok":"still here"}"#
        );
    }

    #[test]
    fn async_handlers_respond_from_a_worker_thread() {
        let caller = std::thread::current().id();
        let mut reg = Registry::new();
        reg.add_async("slow", move |_h, _p| {
            assert_ne!(
                std::thread::current().id(),
                caller,
                "an async handler must not run on the caller's thread"
            );
            Ok(json!("done"))
        });

        let (tx, rx) = mpsc::channel();
        reg.dispatch_deferred(&empty_handle(), "slow", Value::Null, move |body| {
            let _ = tx.send(body);
        });
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            r#"{"ok":"done"}"#
        );
    }

    #[test]
    fn a_panicking_async_handler_is_isolated_too() {
        let mut reg = Registry::new();
        reg.add_async("boom", |_h, _p| panic!("async kaboom"));

        let (tx, rx) = mpsc::channel();
        reg.dispatch_deferred(&empty_handle(), "boom", Value::Null, move |body| {
            let _ = tx.send(body);
        });
        let body = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["error"]["message"], "command panicked: async kaboom");
        assert_eq!(parsed["error"]["kind"], "panic");
    }

    #[test]
    fn error_messages_are_json_escaped() {
        let mut reg = Registry::new();
        reg.add("q", |_h, _p| Err(Error::Handler("say \"hi\"\nnow".into())));
        let out = reg.dispatch(&empty_handle(), "q", Value::Null);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["error"]["message"], "say \"hi\"\nnow");
    }

    #[test]
    fn endpoint_paths_are_recognised_and_decoded() {
        assert_eq!(method_from_path("/__kiln/greet").as_deref(), Some("greet"));
        assert_eq!(
            method_from_path("/__kiln/save%20file").as_deref(),
            Some("save file")
        );
        assert_eq!(method_from_path("/__kiln/"), None);
        assert_eq!(method_from_path("/index.html"), None);
        assert_eq!(method_from_path("/"), None);
    }
}
