//! Native file dialogs, exposed as commands.
//!
//! Registered names, all async because a dialog blocks until the user answers:
//!
//! | Command | Returns |
//! | --- | --- |
//! | `dialog.open` | `{"path": string} \| null` |
//! | `dialog.open_many` | `{"paths": string[]} \| null` |
//! | `dialog.save` | `{"path": string} \| null` |
//! | `dialog.folder` | `{"path": string} \| null` |
//!
//! Payload accepts `title`, `file_name`, `directory`, and `filters` as
//! `[{"name": "Images", "extensions": ["png"]}]`. Returning `null` on cancel is
//! deliberate: it is the one shape a caller can branch on without catching.
//!
//! Desktop goes through `rfd`, which blocks the calling thread. Android has no
//! `rfd`: the picker is an Android intent started by the activity, so the request
//! is described as JSON, handed to the activity, and the answer comes back as the
//! same JSON shape. Both entry points are async, so neither blocks the window.

use serde_json::Value;

#[cfg(any(not(any(target_os = "android", target_os = "ios")), test))]
use serde_json::json;

use crate::Registry;

/// Register every dialog command.
///
/// These are registered async (`add_async`) even though nothing here blocks:
/// the desktop backend blocks the worker thread it runs on, and the Android
/// backend parks that thread on a channel until the activity answers. Registering
/// them synchronously would hold the UI thread for the same interval.
pub fn register(registry: &mut Registry) {
    registry.add_async("dialog.open", |_app, payload| open(&payload, "open"));
    registry.add_async("dialog.open_many", |_app, payload| {
        open(&payload, "open_many")
    });
    registry.add_async("dialog.save", |_app, payload| open(&payload, "save"));
    registry.add_async("dialog.folder", |_app, payload| open(&payload, "folder"));
}

/// One dialog command, from payload to the reply the page sees.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn open(payload: &Value, kind: &str) -> Result<Value, crate::Error> {
    let dialog = build(payload);
    let picked: Option<Value> = match kind {
        "open" => dialog
            .pick_file()
            .as_deref()
            .map(|path| json!({ "path": path_string(path) })),
        "open_many" => dialog.pick_files().map(
            |paths| json!({ "paths": paths.iter().map(|p| path_string(p)).collect::<Vec<_>>() }),
        ),
        "save" => dialog
            .save_file()
            .as_deref()
            .map(|path| json!({ "path": path_string(path) })),
        "folder" => dialog
            .pick_folder()
            .as_deref()
            .map(|path| json!({ "path": path_string(path) })),
        other => {
            return Err(crate::Error::Handler(format!(
                "unknown dialog kind {other}"
            )));
        }
    };
    Ok(picked.unwrap_or(Value::Null))
}

/// One dialog command, from payload to the reply the page sees.
///
/// The request is the payload plus the `kind`, which is what the activity needs
/// to know in order to pick the intent. The answer is already the reply shape,
/// because the activity builds it from the same JSON, so it is passed through
/// unread rather than parsed and rebuilt.
#[cfg(target_os = "android")]
fn open(payload: &Value, kind: &str) -> Result<Value, crate::Error> {
    let mut request = payload.clone();
    if let Some(object) = request.as_object_mut() {
        object.insert("kind".to_owned(), Value::from(kind));
    }
    let request = request.to_string();

    match crate::platform::pick_dialog(&request) {
        Ok(Some(response)) => Ok(serde_json::from_str(&response).unwrap_or(Value::Null)),
        Ok(None) => Ok(Value::Null),
        Err(message) => Err(crate::Error::Handler(format!(
            "dialog {kind} failed: {message}"
        ))),
    }
}

/// iOS has no system file picker a webview app may drive: `UIDocumentPicker`
/// needs the Swift side, which the IOS-PLAN Phase I-3 template owns. The
/// honest answer until then is an error, not a silent `null` a page would
/// read as “nothing selected”.
#[cfg(target_os = "ios")]
fn open(_payload: &Value, kind: &str) -> Result<Value, crate::Error> {
    Err(crate::Error::Handler(format!(
        "dialog {kind} is not available on iOS yet"
    )))
}

/// Paths cross the IPC boundary as UTF-8 strings. A path that is not valid UTF-8
/// cannot be represented in JSON at all, so it is reported rather than mangled
/// into replacement characters that would name a different file.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn path_string(path: &std::path::Path) -> String {
    match path.to_str() {
        Some(text) => text.to_owned(),
        None => {
            crate::reporting::record(
                &format!("non-UTF-8 dialog path: {}", path.display()),
                "dialogs",
            );
            String::new()
        }
    }
}

/// Apply the payload's optional settings to a fresh dialog.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn build(payload: &Value) -> rfd::FileDialog {
    let mut dialog = rfd::FileDialog::new();

    if let Some(title) = payload.get("title").and_then(Value::as_str) {
        dialog = dialog.set_title(title);
    }
    if let Some(name) = payload.get("file_name").and_then(Value::as_str) {
        dialog = dialog.set_file_name(name);
    }
    if let Some(dir) = payload.get("directory").and_then(Value::as_str) {
        dialog = dialog.set_directory(dir);
    }
    for (name, extensions) in parse_filters(payload) {
        if !extensions.is_empty() {
            dialog = dialog.add_filter(name, &extensions);
        }
    }

    dialog
}

/// Parse the payload's filter list without a dialog. Used by `build` and kept
/// separate so the parsing is testable without opening a window.
///
/// Android receives these too, as part of the JSON request: the activity maps an
/// extension to a MIME type, so the filter has to be described the same way.
pub fn parse_filters(payload: &Value) -> Vec<(String, Vec<String>)> {
    payload
        .get("filters")
        .and_then(Value::as_array)
        .map(|filters| {
            filters
                .iter()
                .map(|filter| {
                    let name = filter
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Files")
                        .to_owned();
                    let exts = filter
                        .get("extensions")
                        .and_then(Value::as_array)
                        .map(|list| {
                            list.iter()
                                .filter_map(Value::as_str)
                                .map(|e| e.trim_start_matches('.').to_owned())
                                .collect()
                        })
                        .unwrap_or_default();
                    (name, exts)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_parsed_and_dots_stripped() {
        let payload = json!({
            "filters": [
                { "name": "Images", "extensions": ["png", ".jpg"] },
                { "name": "All", "extensions": ["*"] },
                { "name": "Empty", "extensions": [] },
            ]
        });
        let filters = parse_filters(&payload);
        assert_eq!(filters.len(), 3);
        assert_eq!(filters[0].0, "Images");
        assert_eq!(filters[0].1, vec!["png", "jpg"]);
        assert_eq!(filters[1].1, vec!["*"]);
        assert!(filters[2].1.is_empty());
    }

    #[test]
    fn a_payload_without_filters_yields_none() {
        assert!(parse_filters(&json!({})).is_empty());
        assert!(parse_filters(&json!({ "filters": "nope" })).is_empty());
    }

    #[test]
    fn commands_are_registered_as_async() {
        let mut registry = Registry::new();
        register(&mut registry);
        for name in [
            "dialog.open",
            "dialog.open_many",
            "dialog.save",
            "dialog.folder",
        ] {
            assert!(registry.has(name), "{name} should be registered");
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn paths_convert_to_utf8_strings() {
        let path = std::path::Path::new("/tmp/file.txt");
        assert_eq!(path_string(path), "/tmp/file.txt");
    }

    #[cfg(target_os = "android")]
    #[test]
    fn the_request_carries_the_kind_for_the_activity() {
        // Android's picker is chosen from this field, so a command that lost it
        // would open the wrong intent.
        let request = open(&json!({ "title": "Pick" }), "folder")
            .err()
            .expect("no activity in a test process, so this must report rather than hang");
        assert!(request.to_string().contains("folder"));
    }
}
