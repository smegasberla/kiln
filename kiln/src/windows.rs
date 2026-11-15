//! Window management.
//!
//! Multi-window is a small registry owned by the event loop, since tao window
//! handles are not `Send`. Commands ask for a window by *label*; the request is
//! delivered to the UI thread through the `EventLoopProxy` and the window is
//! created there. Handles therefore never cross threads, and a command handler
//! running anywhere can still say "open a window".

use std::collections::HashMap;

use serde_json::Value;
use tao::{
    dpi::LogicalSize,
    event_loop::EventLoopWindowTarget,
    window::{Window, WindowId},
};
use wry::WebView;

use crate::error::Error;

/// Per-window overrides. Everything is optional; defaults come from the config.
#[derive(Debug, Clone, Default)]
pub struct WindowOptions {
    /// Stable name for the window, used by `__kiln_close` and events. Defaults
    /// to the entry page path.
    pub label: Option<String>,
    pub title: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// Page to load, relative to the served root.
    pub entry: Option<String>,
}

impl WindowOptions {
    /// Build options from a JSON payload, ignoring unknown fields.
    pub fn from_json(payload: &Value) -> Self {
        Self {
            label: payload.get("label").and_then(Value::as_str).map(str::to_owned),
            title: payload.get("title").and_then(Value::as_str).map(str::to_owned),
            width: payload.get("width").and_then(Value::as_f64),
            height: payload.get("height").and_then(Value::as_f64),
            entry: payload.get("entry").and_then(Value::as_str).map(str::to_owned),
        }
    }
}

/// One open window and the webview inside it.
pub struct WindowEntry {
    pub label: String,
    pub window: Window,
    pub webview: WebView,
}

/// Every open window, keyed by tao's window id.
pub type Windows = HashMap<WindowId, WindowEntry>;

/// Insert a window into the registry.
pub fn insert(windows: &mut Windows, id: WindowId, entry: WindowEntry) {
    windows.insert(id, entry);
}

/// Find a window by label.
pub fn find_by_label<'a>(windows: &'a Windows, label: &str) -> Option<&'a WindowEntry> {
    windows.values().find(|e| e.label == label)
}

/// The label of the only window, when exactly one is open. Commands with no
/// window argument use it, which makes single-window apps ergonomic.
pub fn sole_label(windows: &Windows) -> Option<String> {
    if windows.len() == 1 {
        windows.values().next().map(|e| e.label.clone())
    } else {
        None
    }
}

/// Pick the target window label from a payload.
///
/// An explicit `label` always wins. Otherwise the sole open window is used, and
/// with several open this is an error rather than a guess, so a command never
/// silently acts on the wrong window.
pub fn pick_label(labels: &[String], payload: &Value) -> Result<String, Error> {
    if let Some(label) = payload.get("label").and_then(Value::as_str) {
        return Ok(label.to_owned());
    }
    if labels.len() == 1 {
        return Ok(labels[0].clone());
    }
    Err(Error::Handler(match labels.len() {
        0 => "no windows are open".to_owned(),
        _ => "no window label given and more than one window is open".to_owned(),
    }))
}

/// Resolve a payload's `label` field against the open windows.
pub fn resolve_label(windows: &Windows, payload: &Value) -> Result<String, Error> {
    let labels: Vec<String> = windows.values().map(|e| e.label.clone()).collect();
    pick_label(&labels, payload)
}

/// Window size for a window, honoring per-window overrides.
pub fn size_for(options: &WindowOptions, default: (f64, f64)) -> LogicalSize<f64> {
    LogicalSize::new(
        options.width.unwrap_or(default.0),
        options.height.unwrap_or(default.1),
    )
}

/// Where pages are served from, shared by every window.
#[derive(Debug, Clone)]
pub struct Spec {
    pub entry: String,
    pub default_title: String,
    pub default_size: (f64, f64),
    /// Dev-server origin in debug builds, `None` in release.
    pub dev_origin: Option<String>,
}

impl Spec {
    /// The URL for a page path. Absolute URLs pass through, which is what makes
    /// "open a window pointed at the dev server" and "open a window pointed at a
    /// local file" both work.
    pub fn url_for(&self, entry: &str) -> String {
        if entry.starts_with("kiln://") || entry.starts_with("http://") || entry.starts_with("https:") {
            return entry.to_owned();
        }
        match &self.dev_origin {
            Some(dev) => format!("{}/{entry}", dev.trim_end_matches('/')),
            None => format!("{}/{entry}", crate::APP_ORIGIN),
        }
    }

    /// The entry page for a set of options.
    pub fn entry_for(&self, options: &WindowOptions) -> String {
        options.entry.clone().unwrap_or_else(|| self.entry.clone())
    }

    /// The label for a set of options, defaulting to the entry page.
    pub fn label_for(&self, options: &WindowOptions) -> String {
        options
            .label
            .clone()
            .unwrap_or_else(|| self.entry_for(options))
    }

    pub fn title_for(&self, options: &WindowOptions) -> String {
        options
            .title
            .clone()
            .unwrap_or_else(|| self.default_title.clone())
    }
}

/// Create the window itself. The caller supplies a prepared webview builder so
/// protocol registration and the ready hook stay in one place.
pub(crate) fn create_window(
    event_loop: &EventLoopWindowTarget<crate::UserEvent>,
    spec: &Spec,
    options: &WindowOptions,
    build: &dyn Fn(String) -> wry::WebViewBuilder<'static>,
) -> Result<(WindowId, WindowEntry), Error> {
    use tao::window::WindowBuilder;

    let window = WindowBuilder::new()
        .with_title(spec.title_for(options))
        .with_inner_size(size_for(options, spec.default_size))
        .build(event_loop)
        .map_err(|e| Error::Runtime(e.to_string()))?;

    let target = spec.url_for(&spec.entry_for(options));
    let webview = crate::build_webview(build(target), &window)?;

    Ok((
        window.id(),
        WindowEntry {
            label: spec.label_for(options),
            window,
            webview,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec() -> Spec {
        Spec {
            entry: "index.html".to_owned(),
            default_title: "App".to_owned(),
            default_size: (800.0, 600.0),
            dev_origin: None,
        }
    }

    #[test]
    fn urls_are_built_against_the_app_origin() {
        assert_eq!(spec().url_for("index.html"), "kiln://localhost/index.html");
        assert_eq!(spec().url_for("/app/x.html"), "kiln://localhost//app/x.html");
        // Pass-through for absolute URLs.
        assert_eq!(
            spec().url_for("http://127.0.0.1:5177/"),
            "http://127.0.0.1:5177/"
        );
    }

    #[test]
    fn dev_origin_replaces_the_app_origin() {
        let dev = Spec {
            dev_origin: Some("http://127.0.0.1:5177".to_owned()),
            ..spec()
        };
        assert_eq!(dev.url_for("index.html"), "http://127.0.0.1:5177/index.html");
    }

    #[test]
    fn options_parse_from_json_and_ignore_junk() {
        let opts = WindowOptions::from_json(&json!({
            "label": "settings",
            "title": "Settings",
            "width": 400.0,
            "height": 300,
            "unknown": true,
        }));
        assert_eq!(opts.label.as_deref(), Some("settings"));
        assert_eq!(opts.title.as_deref(), Some("Settings"));
        assert_eq!(opts.width, Some(400.0));
        assert_eq!(opts.height, Some(300.0));

        let empty = WindowOptions::from_json(&json!({}));
        assert!(empty.label.is_none() && empty.entry.is_none());
    }

    #[test]
    fn sizes_fall_back_to_the_defaults_per_axis() {
        let size = size_for(
            &WindowOptions {
                width: Some(320.0),
                ..WindowOptions::default()
            },
            (800.0, 600.0),
        );
        assert_eq!(size.width, 320.0);
        assert_eq!(size.height, 600.0);
    }

    #[test]
    fn labels_default_to_the_entry_page() {
        let s = spec();
        let options = WindowOptions::default();
        assert_eq!(s.label_for(&options), "index.html");
        let named = WindowOptions {
            label: Some("settings".to_owned()),
            entry: Some("settings.html".to_owned()),
            ..WindowOptions::default()
        };
        assert_eq!(s.label_for(&named), "settings");
        assert_eq!(s.entry_for(&named), "settings.html");
    }

    #[test]
    fn target_labels_are_never_guessed() {
        // No windows: an error, not a wrong window.
        let err = pick_label(&[], &json!({})).unwrap_err();
        assert!(err.to_string().contains("no windows are open"), "{err}");

        // One window: that window.
        let one = vec!["main".to_owned()];
        assert_eq!(pick_label(&one, &json!({})).unwrap(), "main");

        // Two windows with no label: refuse rather than pick one.
        let two = vec!["main".to_owned(), "settings".to_owned()];
        let err = pick_label(&two, &json!({})).unwrap_err();
        assert!(err.to_string().contains("more than one window"), "{err}");

        // An explicit label always wins, even when several are open.
        assert_eq!(
            pick_label(&two, &json!({ "label": "settings" })).unwrap(),
            "settings"
        );
    }
}
