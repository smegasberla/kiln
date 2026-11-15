//! A minimal Android Kiln app.
//!
//! The only platform-specific line is the binding below; the app body is the same
//! `App` a desktop build uses.

use kiln::serde_json::{json, Value};
use kiln::{App, AssetTable};

static ASSETS: AssetTable = &[("index.html", include_bytes!("../app/src/main/assets/index.html"))];

/// The activity's native entry point.
///
/// Named by hand because it is a JNI symbol, not a Rust function: Java looks for
/// `Java_com_kiln_example_MainActivity_android_1main` (`_1` is the escape for `_`).
#[no_mangle]
pub extern "C" fn Java_com_kiln_example_MainActivity_android_1main(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    android_main();
}

fn android_main() {
    // Installs the JNI entry points the activity calls. Inside the function on
    // purpose: tao's macro expands to a block, which module scope rejects.
    kiln_android::register_jni!(com_kiln, example, Rust, android_main);

    kiln_android::run(
        App::new()
            .assets(ASSETS)
            .command("greet", |_app, payload: Value| {
                let name = payload.get("name").and_then(Value::as_str).unwrap_or("world");
                // Logged on the Rust side so a headless run can prove the command
                // actually executed, not merely that the page rendered.
                println!("KILN_ROUNDTRIP command=greet name={name}");
                Ok(json!(format!("Hello from Android, {name}!")))
            }),
    );
}
