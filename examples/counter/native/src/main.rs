//! Counter: state in Rust, rendering in the page.
//!
//! The interesting bit is that the window owns no application state. `shift`
//! and `reset` return the new counter over IPC, and `emit` pushes the same
//! value to any listener, so a second window or a background task could keep
//! the UI honest.
//!
//! `shift` is registered with `command_typed`: the payload is deserialized into
//! [`ShiftArgs`] before the handler runs, so `{"delta":"2"}` comes back as a
//! `payload` rejection naming the command instead of a silently ignored zero.

use std::sync::{Arc, Mutex};

use kiln::serde_json::json;
use kiln::{App, AppHandle, Error};

mod assets;

/// `shift` arguments.
///
/// `#[serde(crate = ...)]` is what lets the derive find the re-exported crate:
/// an app depends on `kiln` alone, never on `serde` directly.
#[derive(Debug, kiln::serde::Deserialize)]
#[serde(crate = "kiln::serde")]
struct ShiftArgs {
    /// How far to move the counter. Omitted means zero, not an error.
    #[serde(default)]
    delta: i64,
}

/// The reply shape every counter command shares, so the page has one type.
#[derive(Debug, Clone, kiln::serde::Serialize)]
#[serde(crate = "kiln::serde")]
struct Snapshot {
    value: i64,
    history: Vec<i64>,
}

#[derive(Default)]
struct Counter {
    value: i64,
    history: Vec<i64>,
}

impl Counter {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            value: self.value,
            history: self.history.clone(),
        }
    }
}

fn main() -> Result<(), Error> {
    let state = Arc::new(Mutex::new(Counter::default()));

    App::new()
        .config(include_str!("../../kiln.json"))
        .assets(assets::ASSETS)
        // A typed command with no arguments: the payload type is `()`.
        .command_typed("current", {
            let state = Arc::clone(&state);
            move |_app: &AppHandle, _payload: ()| {
                let counter = state.lock().unwrap();
                Ok(counter.snapshot())
            }
        })
        .command_typed("shift", {
            let state = Arc::clone(&state);
            move |app: &AppHandle, args: ShiftArgs| {
                let snapshot = {
                    let mut counter = state.lock().unwrap();
                    counter.value += args.delta;
                    let value = counter.value;
                    counter.history.push(value);
                    counter.snapshot()
                };
                app.emit("count", json!(snapshot));
                Ok(snapshot)
            }
        })
        .command_typed("reset", {
            let state = Arc::clone(&state);
            move |_app: &AppHandle, _payload: ()| {
                let mut counter = state.lock().unwrap();
                counter.value = 0;
                counter.history.clear();
                Ok(counter.snapshot())
            }
        })
        // Async command: a slow "export" that does not block the window. The
        // raw `Value` form is the right one here: an export takes no arguments.
        .async_command("export", {
            let state = Arc::clone(&state);
            move |_app: AppHandle, _payload| {
                std::thread::sleep(std::time::Duration::from_millis(400));
                let counter = state.lock().unwrap();
                let snapshot = counter.snapshot();
                Ok(json!({
                    "value": snapshot.value,
                    "history": snapshot.history,
                    "exported": true
                }))
            }
        })
        .on_ready(|app, window| {
            app.emit_to(Some(window), "ready", json!(true));
        })
        .run()
}
