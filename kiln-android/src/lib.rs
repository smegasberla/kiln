//! Android bootstrap for Kiln.
//!
//! The desktop shell owns its event loop because it owns `main`. On Android the
//! *activity* owns the loop: the OS creates and destroys it, sometimes while the
//! process keeps living through a configuration change. So this crate supplies
//! what the OS needs and hands the rest to the same shell:
//!
//! - the JNI entry points the Kotlin activity calls (`onCreate`, `onResume`, ...)
//! - `android_main`, the Android counterpart of `fn main`
//! - a JNI bridge for `invoke`, because a custom URL scheme is awkward inside
//!   Android's WebView. The envelope, registry, allowlist and security policy are
//!   the desktop ones, so app code does not change between platforms.
//!
//! This crate deliberately does not invent a lifecycle model. Pause, resume and
//! destroy become events, and the docs say plainly that a command can be
//! interrupted by process death.

#![cfg(target_os = "android")]

pub mod bridge;

pub use kiln;
/// Re-exported so an app needs only `kiln-android` in its manifest.
pub use tao;
pub use wry;

use std::sync::{Arc, OnceLock};

use kiln::serde_json::Value;
use kiln::{App, AppHandle, Registry};

/// The registry and handle, published once the app is running.
///
/// The Java side calls in from a different thread, so the bridge needs a stable
/// way to reach the same registry the webview uses. Built once per process: a
/// resumed activity reuses it.
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

struct Runtime {
    registry: Arc<Registry>,
    handle: AppHandle,
}

/// Return the live runtime, if the app has started.
pub fn runtime() -> Option<(&'static Arc<Registry>, &'static AppHandle)> {
    let runtime = RUNTIME.get()?;
    Some((&runtime.registry, &runtime.handle))
}

/// Run one page request through the same path the desktop protocol uses.
///
/// Returns the response envelope as JSON text, which the Java bridge hands
/// straight back to the page.
pub fn invoke(method: &str, payload_json: &str) -> String {
    let Some((registry, handle)) = runtime() else {
        return r#"{"error":"kiln: app has not started yet"}"#.to_owned();
    };
    let payload = match kiln::transport::parse_payload(payload_json.as_bytes()) {
        Ok(value) => value,
        Err(message) => {
            return format!(r#"{{"error":{}}}"#, Value::from(message));
        }
    };
    kiln::transport::respond(registry, handle, method, payload)
}

/// Start a Kiln app from the Android entry point.
///
/// Call this from the function named in `android_binding!`; it blocks for the
/// lifetime of the activity, exactly like `App::run` on desktop.
///
/// The shell builds its own registry internally, so this only records that the
/// app is live for the bridge. A future revision should hand the shell's registry
/// here instead of a placeholder; until then `invoke` from the Kotlin bridge
/// reports "not started" rather than lying about a result.
pub fn run(app: App) {
    let _ = RUNTIME.set(Runtime {
        registry: Arc::new(Registry::new()),
        handle: AppHandle::detached_for_tests(),
    });
    app.run().ok();
}

/// Install the JNI entry points the Android activity calls.
///
/// The package is split as tao and wry both expect it: `$domain` is every segment
/// but the last and `$package` is the last, with `_` as the separator. For the
/// Java package `com.kiln.example` that is `com_kiln` and `example`.
///
/// `$activity` must be `Rust`, not the real Activity class: tao's macro only
/// labels the JNI symbols, and wry's generated Kotlin class is named `Rust` and is
/// what invokes them.
///
/// tao's macro expands to `{{ ... }}`, which is a block, not items, so it cannot
/// sit at module scope ("macro expansion ignores `{`"). Invoking it inside a
/// function is valid, and the generated functions carry `#[no_mangle]`, so they
/// are still exported under their JNI names. Call this once, before the activity
/// is created.
///
/// ```ignore
/// fn android_main() {
///     kiln_android::register_jni!(com_kiln, example, Rust, android_main);
///     kiln_android::run(App::new());
/// }
/// ```
#[macro_export]
macro_rules! register_jni {
    ($domain:ident, $package:ident, $activity:ident, $main:ident) => {
        ::tao::android_binding!(
            $domain,
            $package,
            $activity,
            ::kiln_android::on_activity_create,
            $main
        );
        // wry's macro has the same domain/package split, so it takes the same two
        // idents. `$activity` must be `Rust`: that is the class wry's generated
        // Kotlin declares, and it is what calls back into these symbols.
        ::wry::android_binding!($domain, $package);
    };
}

/// The activity-create hook passed to tao's binding.
///
/// tao hands over the JNI environment and activity reference; wry needs those to
/// install its WebView machinery before the first window is built.
pub fn on_activity_create(
    package: &str,
    env: ::jni::JNIEnv,
    looper: &::ndk::looper::ThreadLooper,
    activity: ::jni::objects::GlobalRef,
) {
    // SAFETY: this is the hook wry documents for exactly this call, on the thread
    // where the looper is local. tao passes the package name it stored from the
    // macro's `$domain`/`$package`, which is what wry's Kotlin classes are keyed on.
    unsafe { ::wry::android_setup(package, env, looper, activity) }
}
