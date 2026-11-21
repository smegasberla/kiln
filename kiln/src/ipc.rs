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
//!
//! # Layout
//!
//! | Module | Holds |
//! | --- | --- |
//! | [`registry`] | the table, the allowlists, and the worker bound |
//! | [`register`] | the `add*` surface an app calls |
//! | [`dispatch`] | one request, from method name to response body |
//! | [`handler`] | the handler kinds, and panic isolation |
//! | [`envelope`] | the bytes a page receives, on either side |
//! | [`metrics`] | per-command latency samples |
//! | [`worker`] | slots that bound thread-per-call handlers |
//! | [`path`] | the endpoint path decoder |
//!
//! The split is by concern, not by size: `dispatch` is where the threat model is
//! enforced, and reading it should not require scrolling past registration.

mod dispatch;
mod envelope;
mod handler;
mod metrics;
mod path;
mod registry;
mod register;
mod worker;

#[cfg(test)]
mod fuzz;
#[cfg(test)]
mod tests;

/// The path prefix the shell reserves for the invoke endpoint.
pub const ENDPOINT_PREFIX: &str = "/__kiln/";

pub use path::method_from_path;
pub use registry::Registry;

#[cfg(feature = "hot-reload")]
pub(crate) use envelope::error_envelope;
pub(crate) use handler::panic_message;
