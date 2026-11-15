//! Android entry point for Counter.
//!
//! The counterpart of `native/src/main.rs`: same app, same commands. The only
//! difference is that the activity owns the event loop instead of `main`.
//!
//! `register_jni!` installs the symbols wry's generated Kotlin calls. It must run
//! inside a function because tao's macro expands to a block, which Rust rejects at
//! module scope; `#[no_mangle]` still exports from within a function.

use kiln::serde_json::{json, Value};
use kiln::{App, AppHandle};

mod assets;

/// The activity's native entry point, named as JNI expects.
#[no_mangle]
pub extern "C" fn Java_com_kiln_counter_MainActivity_android_1main(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    android_main();
}

fn android_main() {
    kiln_android::register_jni!(com_kiln, counter, Rust, android_main);

    // The same command set as `native/src/main.rs`. The frontend is shared, so a
    // command registered on only one platform is a missing command at runtime;
    // `kiln android build` warns when the two lists differ.
    kiln_android::run(
        App::new()
            .config(include_str!("../../kiln.json"))
            .assets(assets::ASSETS)
            .command("greet", |_app, payload: Value| {
                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("world");
                Ok(json!(format!("Hello, {name}! (from Rust)")))
            })
            // Async work runs off the UI thread, so a slow job cannot freeze the
            // page while the reply is pending.
            .async_command("slow_task", |_app, payload: Value| {
                let ms = payload.get("ms").and_then(Value::as_u64).unwrap_or(500);
                std::thread::sleep(std::time::Duration::from_millis(ms));
                Ok(json!({ "slept_ms": ms }))
            })
            // Android is one window by design, so this reports why rather than
            // silently doing nothing: a second window has nowhere to go.
            .command("open_settings", |_app: &AppHandle, _payload| {
                Err(kiln::Error::Handler(
                    "second windows are desktop-only; use an in-page panel on Android"
                        .to_owned(),
                ))
            }),
    )
}
