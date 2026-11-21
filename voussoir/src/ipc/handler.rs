//! The handler kinds, and the isolation that keeps a panic inside one command.
//!
//! Handlers are called from inside wry's FFI protocol callback, where unwinding
//! across the C boundary is undefined behaviour, so a panic there aborts the
//! process. Catching here means an app with a bug in one command reports the
//! error to the page and keeps the window, and any unsaved work, alive.

use std::panic::{self, AssertUnwindSafe};

use serde_json::Value;

use crate::{AppHandle, Error};

pub(super) type SyncHandler = Box<dyn Fn(&AppHandle, Value) -> Result<Value, Error> + Send + Sync>;
/// Async handlers are shared rather than borrowed so a worker thread can own a
/// reference to one after `dispatch_deferred` returns.
pub(super) type AsyncHandler =
    std::sync::Arc<dyn Fn(AppHandle, Value) -> Result<Value, Error> + Send + Sync>;

/// Deferred handlers own their completion callback, so they can answer after
/// arbitrary work and from any thread or task.
#[cfg(feature = "rt")]
pub(super) type DeferredHandler =
    Box<dyn Fn(AppHandle, Value, Box<dyn FnOnce(String) + Send>) + Send + Sync>;

pub(super) enum Handler {
    Sync(SyncHandler),
    Async(AsyncHandler),
    /// Owns its completion callback, so it can answer after arbitrary work and
    /// from any thread or task. The async runtime registers here; nothing about
    /// the envelope changes.
    #[cfg(feature = "rt")]
    Deferred(DeferredHandler),
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
pub(super) fn run_isolated(
    handler: &SyncHandler,
    handle: &AppHandle,
    payload: Value,
) -> Result<Value, Error> {
    match panic::catch_unwind(AssertUnwindSafe(|| handler(handle, payload))) {
        Ok(result) => result,
        Err(payload) => Err(report_panic(payload.as_ref())),
    }
}

pub(super) fn run_isolated_async(
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
pub(super) fn decode_args<P>(payload: Value) -> Result<P, Error>
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
pub(super) fn report_panic(payload: &(dyn std::any::Any + Send)) -> Error {
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
