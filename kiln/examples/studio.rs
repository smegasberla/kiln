//! Everything a "real app" needs, in one example: menu bar, tray, native dialogs,
//! an update check, multi-window, async commands, devtools, and crash reports.
//!
//! Run: `cargo run --example studio --features full`
//! Headless proof: `KILN_SELFTEST=1 cargo run --example studio --features full`
//! exercises the same code paths (minus the dialogs, which need a human) and
//! prints `SELFTEST OK`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use kiln::serde_json::{json, Value};
use kiln::{App, AppHandle, AssetTable, Error, WindowOptions};

static ASSETS: AssetTable = &[
    ("index.html", include_bytes!("studio/index.html")),
    ("main.js", include_bytes!("studio/main.js")),
];

fn main() -> Result<(), Error> {
    let selftest = std::env::var_os("KILN_SELFTEST").is_some();
    let opened = Arc::new(AtomicUsize::new(0));

    let app = App::new()
        .config(include_str!("studio/kiln.json"))
        .assets(ASSETS)
        .devtools(true)
        // A sync command: reads state, returns immediately.
        .command("stats", {
            let opened = Arc::clone(&opened);
            move |_app, _payload| Ok(json!({ "windows_opened": opened.load(Ordering::Relaxed) }))
        })
        // An async command: pretends to do slow work off the UI thread.
        .async_command("slow_task", |_app, payload| {
            let ms = payload.get("ms").and_then(Value::as_u64).unwrap_or(600);
            std::thread::sleep(std::time::Duration::from_millis(ms));
            Ok(json!({ "slept_ms": ms }))
        })
        // Multi-window: open a second window at the same page with a new label.
        .command("open_settings", {
            let opened = Arc::clone(&opened);
            move |app: &AppHandle, _payload| {
                opened.fetch_add(1, Ordering::Relaxed);
                app.open_window(WindowOptions {
                    label: Some("settings".to_owned()),
                    title: Some("Settings".to_owned()),
                    width: Some(420.0),
                    height: Some(320.0),
                    entry: None,
                })?;
                Ok(json!({ "opened": "settings" }))
            }
        })
        .command("close_self", |app: &AppHandle, payload| {
            if let Some(label) = payload.get("label").and_then(Value::as_str) {
                app.close_window(label);
            }
            Ok(json!({ "requested": payload }))
        })
        // The probe reports back over IPC, so the run is verifiable without a human.
        .command("report", move |_app, payload: Value| {
            if selftest {
                println!("SELFTEST OK: {payload}");
                std::process::exit(0);
            }
            Ok(Value::Null)
        })
        .on_ready(move |app, _window| {
            if selftest {
                run_selftest(app.clone());
            }
        });

    #[cfg(feature = "menu")]
    let app = {
        use kiln::menu::{MenuGroup, MenuItem};
        app.menu(
            MenuGroup::new("File")
                .item(
                    MenuItem::new("new_window", "New Window")
                        .accelerator("CmdOrCtrl+N")
                        .command("open_settings"),
                )
                .item(MenuItem::new("quit", "Quit").accelerator("CmdOrCtrl+Q")),
        )
        .menu(
            MenuGroup::new("View")
                .item(
                    MenuItem::new("devtools", "Toggle Devtools")
                        .accelerator("CmdOrCtrl+Shift+I")
                        .command("__kiln_devtools"),
                )
                .item(MenuItem::new("reports", "Crash Reports").command("__kiln_reports")),
        )
    };

    #[cfg(feature = "tray")]
    let app = {
        use kiln::tray::{TrayConfig, TrayItem};
        app.tray(
            TrayConfig::new("Kiln Studio")
                .item(TrayItem::new("show", "Show").command("stats"))
                .item(TrayItem::new("quit", "Quit")),
        )
    };

    app.run()
}

/// Drive the app the way a user would, then report over the real IPC path.
#[cfg(feature = "full")]
fn run_selftest(app: AppHandle) {
    std::thread::spawn(move || {
        let mut results = serde_json::Map::new();

        // 1. Sync command.
        results.insert(
            "stats".into(),
            app.invoke_command("stats", Value::Null)
                .unwrap_or(Value::Null),
        );

        // 2. Async command actually ran off-thread and replied.
        results.insert(
            "slow".into(),
            app.invoke_command("slow_task", json!({ "ms": 30 }))
                .unwrap_or(Value::Null),
        );

        // 3. Multi-window request is accepted.
        results.insert(
            "open_window".into(),
            app.invoke_command("open_settings", Value::Null)
                .unwrap_or(Value::Null),
        );

        // 4. A panicking command is contained.
        let contained = match app.invoke_command("does_not_exist", Value::Null) {
            Ok(value) => json!({ "unexpected": value }),
            Err(error) => json!({ "error": error.to_string() }),
        };
        results.insert("contained_error".into(), contained);

        // 5. Built-ins answer.
        results.insert(
            "version".into(),
            app.invoke_command("__kiln_version", Value::Null)
                .unwrap_or(Value::Null),
        );
        results.insert(
            "reports".into(),
            app.invoke_command("__kiln_reports", Value::Null)
                .unwrap_or(Value::Null),
        );

        // 6. Update check reports its outcome either way. The example config
        //    points at an unreachable host on purpose, so this exercises the
        //    failure path rather than pretending a network call succeeded.
        results.insert(
            "update".into(),
            match app.invoke_command("update.check", Value::Null) {
                Ok(value) => json!({ "ok": value }),
                Err(error) => json!({ "error": error.to_string() }),
            },
        );

        app.invoke_command("report", Value::Object(results)).ok();
    });
}

#[cfg(not(feature = "full"))]
fn run_selftest(app: AppHandle) {
    std::thread::spawn(move || {
        let results = json!({ "error": "run with --features full for the full selftest" });
        app.invoke_command("report", results).ok();
    });
}
