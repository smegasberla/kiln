//! The single config file: `kiln.json`.
//!
//! Code-first defaults mean a minimal app needs none of it. Every field is
//! optional; unknown fields are ignored so a newer config does not break an
//! older shell.

use std::collections::HashMap;
use serde_json::Value;

const DEFAULT_NAME: &str = "Kiln App";

/// Window and identity settings for one app.
#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub title: String,
    pub width: f64,
    pub height: f64,
    /// Page loaded at startup, relative to the served root.
    pub entry: String,
    /// Optional allowlist. When present, only these commands may be dispatched,
    /// even if more were registered. `None` means "everything registered".
    pub commands: Option<Vec<String>>,
    /// Per-window allowlists keyed by window label. A window named here can
    /// only call commands from the intersection of its list and the global
    /// allowlist, so a `kiln.json` entry can only ever narrow a window's
    /// surface, never widen it.
    pub window_commands: HashMap<String, Vec<String>>,
    /// Update endpoint, read by the `updater` feature. `None` disables checks.
    pub update_url: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: DEFAULT_NAME.to_owned(),
            title: DEFAULT_NAME.to_owned(),
            width: 800.0,
            height: 600.0,
            entry: "index.html".to_owned(),
            commands: None,
            window_commands: HashMap::new(),
            update_url: None,
        }
    }
}

impl Config {
    /// Parse a `kiln.json` string on top of the defaults.
    ///
    /// Returns the JSON error message rather than a crate error type so it can
    /// be surfaced with the file name that failed.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| format!("kiln.json is not valid JSON: {e}"))?;
        let Value::Object(map) = value else {
            return Err("kiln.json must be a JSON object".to_owned());
        };

        let mut config = Self::default();

        if let Some(name) = map.get("name").and_then(Value::as_str) {
            config.name = name.to_owned();
            config.title = name.to_owned();
        }
        if let Some(title) = map.get("title").and_then(Value::as_str) {
            config.title = title.to_owned();
        }
        if let Some(w) = map.get("width").and_then(Value::as_f64) {
            config.width = w;
        }
        if let Some(h) = map.get("height").and_then(Value::as_f64) {
            config.height = h;
        }
        if let Some(entry) = map.get("entry").and_then(Value::as_str) {
            config.entry = entry.trim_start_matches('/').to_owned();
        }
        if let Some(url) = map
            .get("update")
            .and_then(|u| u.get("url"))
            .and_then(Value::as_str)
        {
            config.update_url = Some(url.to_owned());
        }
        if let Some(list) = map.get("commands").and_then(Value::as_array) {
            let names = list
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            config.commands = Some(names);
        }
        if let Some(windows) = map.get("windows").and_then(Value::as_object) {
            for (label, spec) in windows {
                if let Some(list) = spec.get("commands").and_then(Value::as_array) {
                    let names = list
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                    config.window_commands.insert(label.clone(), names);
                }
            }
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_need_no_config() {
        let c = Config::default();
        assert_eq!(c.title, "Kiln App");
        assert_eq!((c.width, c.height), (800.0, 600.0));
        assert!(c.commands.is_none());
    }

    #[test]
    fn name_becomes_title_unless_overridden() {
        let c = Config::from_json(r#"{"name":"Notes"}"#).unwrap();
        assert_eq!(c.title, "Notes");

        let c = Config::from_json(r#"{"name":"Notes","title":"Notes - main"}"#).unwrap();
        assert_eq!(c.title, "Notes - main");
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let c = Config::from_json(r#"{"name":"X","plugin_registry":{"a":1},"future":true}"#).unwrap();
        assert_eq!(c.name, "X");
    }

    #[test]
    fn command_allowlist_parses() {
        let c = Config::from_json(r#"{"commands":["greet","save"]}"#).unwrap();
        assert_eq!(c.commands.unwrap(), vec!["greet", "save"]);
    }

    #[test]
    fn per_window_allowlists_parse_by_label() {
        let c = Config::from_json(
            r#"{"commands":["greet","save"],"windows":{"preview":{"commands":["greet"]}}}"#,
        )
        .unwrap();
        assert_eq!(c.window_commands["preview"], vec!["greet"]);
        // A window without an entry falls back to the global list.
        assert!(!c.window_commands.contains_key("main"));
        // A window entry without a commands array is ignored.
        let c = Config::from_json(r#"{"windows":{"preview":{"width":300}}}"#).unwrap();
        assert!(c.window_commands.is_empty());
    }

    #[test]
    fn entry_defaults_and_normalises() {
        assert_eq!(Config::default().entry, "index.html");
        let c = Config::from_json(r#"{"entry":"/app/index.html"}"#).unwrap();
        assert_eq!(c.entry, "app/index.html");
    }

    #[test]
    fn the_update_endpoint_parses_and_defaults_to_none() {
        assert!(Config::default().update_url.is_none());

        let c = Config::from_json(r#"{"update":{"url":"https://example.com/v.json"}}"#).unwrap();
        assert_eq!(c.update_url.as_deref(), Some("https://example.com/v.json"));

        // A malformed update block is ignored rather than fatal.
        let c = Config::from_json(r#"{"update":{"url":42}}"#).unwrap();
        assert!(c.update_url.is_none());
    }

    #[test]
    fn invalid_json_reports_clearly() {
        let err = Config::from_json("{,}").unwrap_err();
        assert!(err.contains("kiln.json"), "{err}");

        let err = Config::from_json("[1,2]").unwrap_err();
        assert!(err.contains("JSON object"), "{err}");
    }
}
