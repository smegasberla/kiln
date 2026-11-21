//! Counter - a Voussoir desktop app.
//!
//! Everything shipped is here: the windows, the commands, and the embedded
//! frontend bytes. `cargo build --release` produces the whole app.

use voussoir::serde_json::{json, Value};
use voussoir::{App, AppHandle, Error, WindowOptions};

mod assets;

fn main() -> Result<(), Error> {
    App::new()
        .config(include_str!("../../voussoir.json"))
        .assets(assets::ASSETS)
        // Sync command: reads state and returns immediately.
        .command("greet", |_app, payload: Value| {
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("world");
            Ok(json!(format!("Hello, {name}! (from Rust)")))
        })
        // Async command: runs off the UI thread, so a slow job cannot freeze
        // the window. Replace the sleep with real work.
        .async_command("slow_task", |_app, payload: Value| {
            let ms = payload.get("ms").and_then(Value::as_u64).unwrap_or(500);
            std::thread::sleep(std::time::Duration::from_millis(ms));
            Ok(json!({ "slept_ms": ms }))
        })
        // Same name and shape as the Android crate's: `voussoir android build` checks
        // the two command lists agree, and a page shared between them calls it.
        .command("note", |_app, payload: Value| {
            let message = payload
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            println!("page note: {message}");
            Ok(Value::Null)
        })
        // Multi-window: open a second window with its own label.
        .command("open_settings", |app: &AppHandle, _payload| {
            app.open_window(WindowOptions {
                label: Some("settings".to_owned()),
                title: Some("Settings".to_owned()),
                width: Some(420.0),
                height: Some(320.0),
                entry: None,
            allowlist: None,
            })?;
            Ok(Value::Null)
        })
        // Keep devtools available in the build your users run (F12).
        .devtools(true)
        .run()
}
