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

use std::collections::HashMap;

use serde_json::Value;

use crate::{AppHandle, Error};

/// The path prefix the shell reserves for the invoke endpoint.
pub const ENDPOINT_PREFIX: &str = "/__kiln/";

type Handler = Box<dyn Fn(&AppHandle, Value) -> Result<Value, Error> + Send + Sync>;

/// Name -> handler table plus the optional config allowlist.
pub struct Registry {
    handlers: HashMap<String, Handler>,
    allow: Option<Vec<String>>,
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
        }
    }

    pub fn set_allowlist(&mut self, allow: Option<Vec<String>>) {
        self.allow = allow;
    }

    /// Register a handler. Registering the same name twice replaces it.
    pub fn add<F>(&mut self, method: &str, handler: F)
    where
        F: Fn(&AppHandle, Value) -> Result<Value, Error> + Send + Sync + 'static,
    {
        self.handlers.insert(method.to_owned(), Box::new(handler));
    }

    /// Names that can actually be called, sorted, for diagnostics.
    pub fn callable(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .handlers
            .keys()
            .filter(|name| {
                self.allow
                    .as_ref()
                    .is_none_or(|allow| allow.iter().any(|a| a == *name))
            })
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names
    }

    /// Run one request and produce the response body.
    ///
    /// Errors are converted into the envelope rather than propagated, so the
    /// page always gets valid JSON it can act on, and a handler failure never
    /// takes down the window.
    pub fn dispatch(&self, handle: &AppHandle, method: &str, payload: Value) -> String {
        let result = self.invoke(handle, method, payload);
        match result {
            Ok(value) => format!(r#"{{"ok":{}}}"#, value),
            Err(error) => {
                let message = serde_json::to_string(&error.to_string())
                    .unwrap_or_else(|_| "\"command failed\"".to_owned());
                format!(r#"{{"error":{message}}}"#)
            }
        }
    }

    fn invoke(&self, handle: &AppHandle, method: &str, payload: Value) -> Result<Value, Error> {
        if let Some(allow) = &self.allow {
            if !allow.iter().any(|a| a == method) {
                return Err(Error::UnknownMethod(method.to_owned()));
            }
        }
        let handler = self
            .handlers
            .get(method)
            .ok_or_else(|| Error::UnknownMethod(method.to_owned()))?;
        handler(handle, payload)
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
