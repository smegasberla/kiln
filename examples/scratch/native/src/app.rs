//! The app, platform-independent.
//!
//! Both shells build the same `App` from here: `native/src/main.rs` runs it from
//! `main`, and `android/src/lib.rs` runs it from the activity. That is not tidiness
//! for its own sake. A command registered on one shell and not the other is an
//! "unknown command" at runtime on the platform nobody tested, and the only way to
//! make that impossible rather than merely checked is to have one list.
//!
//! What the shells still differ in is the parts that are genuinely platform
//! differences: the desktop has a menu bar, a tray, and can open a second window;
//! Android has one window, a `PopupMenu` the page raises, and a notification.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kiln::serde_json::{json, Value};
use kiln::{App, AppHandle};

/// Everything the app knows, held in one place.
///
/// A `Mutex` rather than a channel because a command may be called from the IPC
/// thread or from one of `async_command`'s workers, and both need the same data.
/// The lock is never held across I/O except in `save`, which is deliberate: the
/// write has to see the snapshot it is about to put on disk, and a document is
/// small enough that copying it twice would be the more expensive mistake.
pub struct Scratch {
    documents: BTreeMap<String, Document>,
    /// Which document the page is looking at.
    open: Option<String>,
    /// The directory notes live in, resolved once so a failed lookup is reported
    /// at startup rather than as a mysterious empty list later.
    dir: Option<PathBuf>,
}

struct Document {
    title: String,
    body: String,
    /// Milliseconds since the epoch, kept as a number so the page can format it.
    modified: u64,
}

impl Document {
    fn summary(&self) -> Value {
        // Deliberately without `body`: the list is what the page renders for a
        // sidebar, and shipping every note's text on every change would make the
        // list's cost proportional to the corpus rather than to its size.
        json!({
            "title": self.title,
            "modified": self.modified,
            "words": self.body.split_whitespace().count(),
            "characters": self.body.chars().count(),
        })
    }
}

/// A note's title is its first non-empty line, trimmed of leading `#`.
///
/// Derived rather than stored separately so there is one source of truth: the
/// user edits text, and the title follows. A note that is empty keeps the name it
/// was created with, because otherwise the sidebar entry would be blank and
/// unclickable.
fn title_of(body: &str, fallback: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('#').trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.chars().take(60).collect())
        .unwrap_or_else(|| fallback.to_owned())
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Words, characters, and lines, computed where the text lives.
///
/// In Rust rather than in the page because it is the one thing in this app that
/// scales with the document rather than with the window, and it is what the
/// status bar and the tray tooltip both show.
fn statistics(body: &str) -> Value {
    json!({
        "words": body.split_whitespace().count(),
        "characters": body.chars().count(),
        "lines": body.lines().count(),
    })
}

impl Scratch {
    /// Load the store from `dir`, or from the platform data directory.
    ///
    /// Files are one per note, named after the note's key, so a note is readable
    /// and editable outside the app. That is a deliberate choice against a single
    /// database file: a plain-text notebook whose contents need this app to read
    /// is a worse deal for the user than one that does not.
    fn load(app_name: &str) -> Self {
        let dir = kiln::data_dir(app_name);
        let mut documents = BTreeMap::new();

        if let Some(dir) = &dir {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                        continue;
                    }
                    let Ok(body) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    let Some(key) = path.file_stem().and_then(|stem| stem.to_str()) else {
                        continue;
                    };
                    let modified = entry
                        .metadata()
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|elapsed| elapsed.as_millis() as u64)
                        .unwrap_or(0);
                    documents.insert(
                        key.to_owned(),
                        Document {
                            title: title_of(&body, key),
                            body,
                            modified,
                        },
                    );
                }
            }
        }

        // A first run has nothing to open, so it gets something rather than an
        // empty window with no explanation. Written to disk immediately rather
        // than kept in memory: a note the user can see but the directory does not
        // contain is a small lie, and it makes "where are my notes" unanswerable
        // until the first edit.
        if documents.is_empty() {
            let welcome = Document {
                title: "Welcome".to_owned(),
                body: WELCOME.to_owned(),
                modified: now_millis(),
            };
            if let Some(dir) = &dir {
                let _ = std::fs::write(dir.join("welcome.md"), &welcome.body);
            }
            documents.insert("welcome".to_owned(), welcome);
        }

        let open = documents.keys().next().cloned();
        Self {
            documents,
            open,
            dir,
        }
    }

    /// The reply every mutating command returns, so the page never has to ask.
    fn snapshot(&self) -> Value {
        let documents: Vec<Value> = self
            .documents
            .iter()
            .map(|(key, document)| {
                let mut summary = document.summary();
                if let Some(object) = summary.as_object_mut() {
                    object.insert("key".to_owned(), Value::from(key.clone()));
                    object.insert("open".to_owned(), Value::from(self.open.as_deref() == Some(key)));
                }
                summary
            })
            .collect();

        json!({
            "documents": documents,
            "open": self.open,
            "body": self.open.as_deref().and_then(|key| self.documents.get(key)).map(|d| d.body.clone()),
            "directory": self.dir.as_ref().map(|dir| dir.display().to_string()),
        })
    }

    fn next_key(&self, wanted: &str) -> String {
        // A slug rather than a counter: the file name is the note's identity on
        // disk, and "untitled-2.md" says more to someone looking at the directory
        // than "4.md" does.
        let slug: String = wanted
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let base = if slug.is_empty() { "untitled".to_owned() } else { slug };

        if !self.documents.contains_key(&base) {
            return base;
        }
        for suffix in 2.. {
            let candidate = format!("{base}-{suffix}");
            if !self.documents.contains_key(&candidate) {
                return candidate;
            }
        }
        unreachable!("an unbounded range always yields a free name")
    }

    /// Write one note to disk, reporting failure instead of dropping it.
    ///
    /// A note that was typed but not saved is the one loss this app can inflict,
    /// so a failed write is returned to the page as an error rather than reproduced
    /// only in the log.
    fn write(&self, key: &str) -> Result<(), String> {
        let Some(dir) = &self.dir else {
            return Err("this app has no directory to save notes in".to_owned());
        };
        let Some(document) = self.documents.get(key) else {
            return Ok(());
        };
        let path = dir.join(format!("{key}.md"));
        std::fs::write(&path, &document.body)
            .map_err(|error| format!("could not write {}: {error}", path.display()))
    }
}

/// The app both shells run.
pub fn build(app_name: &'static str) -> App {
    let state = Arc::new(Mutex::new(Scratch::load(app_name)));

    App::new()
        .config(include_str!("../../kiln.json"))
        .assets(crate::assets::ASSETS)
        .command("current", {
            let state = Arc::clone(&state);
            move |_app, _payload| Ok(state.lock().unwrap().snapshot())
        })
        .command("open", {
            let state = Arc::clone(&state);
            move |_app, payload: Value| {
                let key = payload.get("key").and_then(Value::as_str).unwrap_or_default();
                let mut scratch = state.lock().unwrap();
                if !scratch.documents.contains_key(key) {
                    return Err(kiln::Error::Handler(format!("no note named {key}")));
                }
                scratch.open = Some(key.to_owned());
                Ok(scratch.snapshot())
            }
        })
        .command("create", {
            let state = Arc::clone(&state);
            move |_app, payload: Value| {
                let wanted = payload.get("title").and_then(Value::as_str).unwrap_or("untitled");
                let mut scratch = state.lock().unwrap();
                let key = scratch.next_key(wanted);
                scratch.documents.insert(
                    key.clone(),
                    Document {
                        title: title_of("", wanted),
                        body: String::new(),
                        modified: now_millis(),
                    },
                );
                scratch.open = Some(key.clone());
                // A note exists on disk before it has content, so the directory
                // listing matches the sidebar even if the app dies immediately.
                scratch.write(&key).map_err(kiln::Error::Handler)?;
                Ok(scratch.snapshot())
            }
        })
        .command("rename", {
            let state = Arc::clone(&state);
            move |_app, payload: Value| {
                let key = payload.get("key").and_then(Value::as_str).unwrap_or_default().to_owned();
                let title = payload.get("title").and_then(Value::as_str).unwrap_or_default().to_owned();
                let mut scratch = state.lock().unwrap();
                let Some(document) = scratch.documents.get_mut(&key) else {
                    return Err(kiln::Error::Handler(format!("no note named {key}")));
                };
                // The title is the first line of the file, so renaming rewrites the
                // file rather than storing a second name somewhere else.
                let heading = format!("# {title}");
                document.body = match document.body.find('\n') {
                    Some(newline) => format!("{heading}{}", &document.body[newline..]),
                    None => heading,
                };
                document.title = title_of(&document.body, &key);
                document.modified = now_millis();
                scratch.write(&key).map_err(kiln::Error::Handler)?;
                Ok(scratch.snapshot())
            }
        })
        .command("delete", {
            let state = Arc::clone(&state);
            move |_app, payload: Value| {
                let key = payload.get("key").and_then(Value::as_str).unwrap_or_default();
                let mut scratch = state.lock().unwrap();
                scratch.documents.remove(key);
                if let Some(dir) = &scratch.dir {
                    let path = dir.join(format!("{key}.md"));
                    let _ = std::fs::remove_file(path);
                }
                if scratch.open.as_deref() == Some(key) {
                    scratch.open = scratch.documents.keys().next().cloned();
                }
                Ok(scratch.snapshot())
            }
        })
        // Async because it may be called per keystroke: the write happens off the
        // UI thread, and the page does not wait for the disk to spin.
        .async_command("save", {
            let state = Arc::clone(&state);
            move |app: AppHandle, payload: Value| {
                let key = payload.get("key").and_then(Value::as_str).unwrap_or_default().to_owned();
                let body = payload.get("body").and_then(Value::as_str).map(str::to_owned);

                let reply = {
                    let mut scratch = state.lock().unwrap();
                    let Some(document) = scratch.documents.get_mut(&key) else {
                        return Err(kiln::Error::Handler(format!("no note named {key}")));
                    };
                    if let Some(body) = body {
                        document.body = body;
                        document.title = title_of(&document.body, &key);
                    }
                    document.modified = now_millis();
                    let statistics = statistics(&document.body);
                    let saved = scratch.write(&key);
                    (scratch.snapshot(), statistics, saved)
                };

                let (snapshot, statistics, saved) = reply;

                // Pushed rather than only returned: the tray tooltip and the
                // sidebar's word count are updated from this, and a second window
                // watching the same note has no other way to hear about it.
                if let Ok(snapshot) = &saved {
                    app.emit("saved", json!({
                        "key": key,
                        "statistics": statistics,
                        "snapshot": snapshot,
                    }));
                }
                saved.map_err(kiln::Error::Handler)?;
                Ok(json!({ "saved": true, "statistics": statistics, "snapshot": snapshot }))
            }
        })
        // Counted on the draft rather than the saved copy, because the status bar
        // has to track what is typed and that is necessarily ahead of the disk. A
        // note the page has not saved yet therefore still counts.
        .command("statistics", |_app, payload: Value| {
            let body = payload.get("body").and_then(Value::as_str).unwrap_or_default();
            Ok(statistics(body))
        })
        // Multi-window is desktop-only, and the page hides the button when it is
        // not, so this is only reachable if a page asks anyway.
        .command("open_second", |app: &AppHandle, _payload| {
            app.open_window(kiln::WindowOptions {
                label: Some("notes".to_owned()),
                title: Some("Scratch notes".to_owned()),
                width: Some(560.0),
                height: Some(640.0),
                entry: None,
            allowlist: None,
            })?;
            Ok(Value::Null)
        })
        .on_ready({
            let state = Arc::clone(&state);
            move |app: &AppHandle, _window: &str| {
                let snapshot = state.lock().unwrap().snapshot();
                app.emit("ready", snapshot);
            }
        })
        .devtools(cfg!(debug_assertions))
}

/// Shared with the page, which explains why it is a constant here and not in TS.
const WELCOME: &str = "# Welcome to Scratch

A notebook that keeps your notes as plain markdown files, so they outlive the app.

- `Ctrl/Cmd+N` starts a note
- `Ctrl/Cmd+S` saves one
- The counter in the corner is Rust counting, not the page

Everything is stored as one `.md` file per note in this app's data directory.";
