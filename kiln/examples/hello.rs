//! A minimal Kiln app: embedded assets, one command round-trip, one event.
//!
//! Run: `cargo run --example hello`
//! Headless proof: `KILN_SELFTEST=1 cargo run --example hello` exits 0 after the
//! JS -> Rust -> JS round-trip prints `SELFTEST OK`.

use kiln::serde_json::{json, Value};
use kiln::{App, AssetTable, Error};

static ASSETS: AssetTable = &[
    ("index.html", include_bytes!("hello/index.html")),
    ("style.css", include_bytes!("hello/style.css")),
    ("main.js", include_bytes!("hello/main.js")),
];

fn main() -> Result<(), Error> {
    let started = std::time::Instant::now();
    let selftest = std::env::var_os("KILN_SELFTEST").is_some();
    let bench = std::env::var_os("KILN_BENCH").is_some();

    App::new()
        .config(include_str!("hello/kiln.json"))
        .assets(ASSETS)
        .command("greet", |_app, payload: Value| {
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("world");
            Ok(json!(format!("Hello, {name}! (from Rust)")))
        })
        .command("report", move |_app, payload: Value| {
            let greeting = payload.get("greeting").and_then(Value::as_str).unwrap_or("");
            if selftest && greeting.contains("from Rust") {
                println!("SELFTEST OK: {greeting}");
                std::process::exit(0);
            }
            Ok(json!(null))
        })
        .on_ready(move |app| {
            if bench {
                println!("READY {}", started.elapsed().as_millis());
                use std::io::Write;
                let _ = std::io::stdout().flush();
                std::process::exit(0);
            }
            std::thread::spawn({
                let app = app.clone();
                move || {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    app.emit("tick", json!(1));
                }
            });
        })
        .run()
}
