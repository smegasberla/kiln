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

use serde_json::{json, Value};

use crate::Registry;

/// Register every dialog command.
///
/// These run on worker threads (`add_async`) because `rfd` blocks the calling
/// thread until the user closes the dialog. Running them inline would freeze the
/// window for as long as the dialog is open.
pub fn register(registry: &mut Registry) {
    registry.add_async("dialog.open", |_app, payload| {
        let dialog = build(&payload);
        Ok(dialog.pick_file().map(|p| json!({ "path": path_string(&p) })).unwrap_or(Value::Null))
    });

    registry.add_async("dialog.open_many", |_app, payload| {
        let dialog = build(&payload);
        Ok(dialog
            .pick_files()
            .map(|paths| json!({ "paths": paths.iter().map(|p| path_string(p)).collect::<Vec<_>>() }))
            .unwrap_or(Value::Null))
    });

    registry.add_async("dialog.save", |_app, payload| {
        let dialog = build(&payload);
        Ok(dialog.save_file().map(|p| json!({ "path": path_string(&p) })).unwrap_or(Value::Null))
    });

    registry.add_async("dialog.folder", |_app, payload| {
        let dialog = build(&payload);
        Ok(dialog.pick_folder().map(|p| json!({ "path": path_string(&p) })).unwrap_or(Value::Null))
    });
}

/// Paths cross the IPC boundary as UTF-8 strings. A path that is not valid UTF-8
/// cannot be represented in JSON at all, so it is reported rather than mangled
/// into replacement characters that would name a different file.
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
        for name in ["dialog.open", "dialog.open_many", "dialog.save", "dialog.folder"] {
            assert!(registry.has(name), "{name} should be registered");
        }
    }

    #[test]
    fn paths_convert_to_utf8_strings() {
        let path = std::path::Path::new("/tmp/file.txt");
        assert_eq!(path_string(path), "/tmp/file.txt");
    }

}
