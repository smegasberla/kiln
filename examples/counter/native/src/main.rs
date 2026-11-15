//! Counter: state in Rust, rendering in the page.
//!
//! The interesting bit is that the window owns no application state. `shift`
//! and `reset` return the new counter over IPC, and `emit` pushes the same
//! value to any listener, so a second window or a background task could keep
//! the UI honest.

use std::sync::{Arc, Mutex};

use kiln::serde_json::{json, Value};
use kiln::{App, AppHandle, Error};

mod assets;

#[derive(Default)]
struct Counter {
    value: i64,
    history: Vec<i64>,
}

impl Counter {
    fn snapshot(&self) -> Value {
        json!({ "value": self.value, "history": self.history })
    }
}

fn main() -> Result<(), Error> {
    let state = Arc::new(Mutex::new(Counter::default()));

    App::new()
        .config(include_str!("../../kiln.json"))
        .assets(assets::ASSETS)
        .command("current", {
            let state = Arc::clone(&state);
            move |_app, _payload| {
                let counter = state.lock().unwrap();
                Ok(counter.snapshot())
            }
        })
        .command("shift", {
            let state = Arc::clone(&state);
            move |app: &AppHandle, payload: Value| {
                let delta = payload.get("delta").and_then(Value::as_i64).unwrap_or(0);
                let snapshot = {
                    let mut counter = state.lock().unwrap();
                    counter.value += delta;
                    let value = counter.value;
                    counter.history.push(value);
                    counter.snapshot()
                };
                app.emit("count", snapshot.clone());
                Ok(snapshot)
            }
        })
        .command("reset", {
            let state = Arc::clone(&state);
            move |_app, _payload| {
                let mut counter = state.lock().unwrap();
                counter.value = 0;
                counter.history.clear();
                Ok(counter.snapshot())
            }
        })
        // Async command: a slow "export" that does not block the window.
        .async_command("export", {
            let state = Arc::clone(&state);
            move |_app, _payload| {
                std::thread::sleep(std::time::Duration::from_millis(400));
                let counter = state.lock().unwrap();
                Ok(json!({
                    "value": counter.value,
                    "history": counter.history,
                    "exported": true
                }))
            }
        })
        .on_ready(|app| {
            app.emit("ready", json!(true));
        })
        .run()
}
