//! The command registry and the wire envelope.
//!
//! One endpoint (`POST kiln://localhost/__kiln/<method>`), one envelope shape
//! in both directions:
//!
//! ```text
//! request  body: <payload as JSON, or null>
//! response body: {"ok": <json>} | {"error": "<message>"}
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
type AsyncHandler =
    std::sync::Arc<dyn Fn(AppHandle, Value) -> Result<Value, Error> + Send + Sync>;

enum Handler {
    Sync(SyncHandler),
    Async(AsyncHandler),
}

/// Name -> handler table plus the optional config allowlist.
pub struct Registry {
    handlers: HashMap<String, Handler>,
    allow: Option<Vec<String>>,
    /// The hot-reloaded commands library. Present only in `kiln dev` builds;
    /// a release build never compiles it.
    #[cfg(feature = "hot-reload")]
    hot: Option<std::sync::Arc<crate::hot::HotLib>>,
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
        self.handlers
            .insert(method.to_owned(), Handler::Async(std::sync::Arc::new(handler)));
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
        rx.recv().unwrap_or_else(|_| error_envelope(&Error::Handler("no response".to_owned())))
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
        // The allowlist gates everything below, the hot library included:
        // `commands` in kiln.json can only ever narrow what is reachable.
        if !self.permitted(method) {
            return done(error_envelope(&Error::UnknownMethod(method.to_owned())));
        }
        match self.handlers.get(method) {
            Some(Handler::Sync(handler)) => {
                done(ok_or_error(run_isolated(handler, handle, payload)))
            }
            Some(Handler::Async(handler)) => {
                let handler = std::sync::Arc::clone(handler);
                let handle = handle.clone();
                std::thread::spawn(move || {
                    done(ok_or_error(run_isolated_async(&handler, handle, payload)));
                });
            }
            None => self.dispatch_unclaimed(method, payload, done),
        }
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
                                error_envelope(&Error::Handler("hot command call failed".to_owned()))
                            });
                    done(body);
                });
            }
            None => done(error_envelope(&Error::UnknownMethod(method.to_owned()))),
        }
    }

    #[cfg(not(feature = "hot-reload"))]
    fn dispatch_unclaimed<F>(&self, method: &str, _payload: Value, done: F)
    where
        F: FnOnce(String) + Send + 'static,
    {
        done(error_envelope(&Error::UnknownMethod(method.to_owned())))
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
fn run_isolated(
    handler: &SyncHandler,
    handle: &AppHandle,
    payload: Value,
) -> Result<Value, Error> {
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

fn ok_or_error(result: Result<Value, Error>) -> String {
    match result {
        Ok(value) => format!(r#"{{"ok":{value}}}"#),
        Err(error) => error_envelope(&error),
    }
}

pub(crate) fn error_envelope(error: &Error) -> String {
    let message = serde_json::to_string(&error.to_string())
        .unwrap_or_else(|_| "\"command failed\"".to_owned());
    format!(r#"{{"error":{message}}}"#)
}

fn report_panic(payload: &(dyn std::any::Any + Send)) -> Error {
    let message = panic_message(payload);
    eprintln!("kiln: handler panicked: {message}");
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
            reg.add(name, move |_h, payload| Ok(json!({ "echo": payload, "name": owned })));
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
        assert!(out.contains(r#""error":"unknown command: nope""#), "{out}");
    }

    #[test]
    fn allowlist_restricts_registered_commands() {
        let mut reg = registry_with(&["a", "b"]);
        reg.set_allowlist(Some(vec!["a".to_owned()]));
        assert_eq!(reg.callable(), vec!["a"]);
        assert!(reg.dispatch(&empty_handle(), "b", Value::Null).contains("error"));
        assert!(reg.dispatch(&empty_handle(), "a", Value::Null).contains(r#""ok""#));
    }

    #[test]
    fn handler_errors_become_error_envelopes() {
        let mut reg = Registry::new();
        reg.add("boom", |_h, _p| Err(Error::Handler("kaboom".into())));
        let out = reg.dispatch(&empty_handle(), "boom", Value::Null);
        assert_eq!(out, r#"{"error":"kaboom"}"#);
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
        let message = parsed["error"].as_str().expect("error envelope expected");
        assert!(message.starts_with("command panicked:"), "{message}");
        assert!(message.contains("index out of bounds"), "{message}");
    }

    #[test]
    fn a_panicking_handler_does_not_poison_later_calls() {
        let mut reg = Registry::new();
        reg.add("boom", |_h, _p| panic!("kaboom"));
        reg.add("fine", |_h, _p| Ok(json!("still here")));

        assert!(reg.dispatch(&empty_handle(), "boom", Value::Null).contains("error"));
        assert_eq!(reg.dispatch(&empty_handle(), "fine", Value::Null), r#"{"ok":"still here"}"#);
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
        assert_eq!(parsed["error"], "command panicked: async kaboom");
    }

    #[test]
    fn error_messages_are_json_escaped() {
        let mut reg = Registry::new();
        reg.add("q", |_h, _p| Err(Error::Handler("say \"hi\"\nnow".into())));
        let out = reg.dispatch(&empty_handle(), "q", Value::Null);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["error"], "say \"hi\"\nnow");
    }

    #[test]
    fn endpoint_paths_are_recognised_and_decoded() {
        assert_eq!(method_from_path("/__kiln/greet").as_deref(), Some("greet"));
        assert_eq!(method_from_path("/__kiln/save%20file").as_deref(), Some("save file"));
        assert_eq!(method_from_path("/__kiln/"), None);
        assert_eq!(method_from_path("/index.html"), None);
        assert_eq!(method_from_path("/"), None);
    }
}
