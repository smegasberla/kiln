//! The bytes a page receives.
//!
//! One shape, built in one place: `{"ok": ...}` on success and
//! `{"error": {"message", "kind", "method"}}` on failure. The `kind` is what a
//! page branches on, and `unknown` deliberately covers both "not registered" and
//! "withheld by an allowlist" so a page cannot probe for commands it may not use.

use serde_json::Value;

use crate::Error;

pub(super) fn ok_or_error(method: &str, result: Result<Value, Error>) -> String {
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
