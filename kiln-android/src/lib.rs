//! Android bootstrap for Kiln.
//!
//! The desktop shell owns its event loop because it owns `main`. On Android the
//! *activity* owns the loop: the OS creates and destroys it, sometimes while the
//! process keeps living through a configuration change. So this crate supplies
//! what the OS needs and hands the rest to the same shell:
//!
//! - the JNI entry points the Kotlin activity calls (`onCreate`, `onResume`, ...)
//! - `android_main`, the Android counterpart of `fn main`
//!
//! Page requests do not cross this crate. Android's WebView makes a custom URL
//! scheme awkward, so `kiln` installs wry's IPC handler on Android and answers
//! `window.ipc` itself; the envelope, registry, allowlist and security policy are
//! the desktop ones. The other direction, Java calling in for a dialog result or a
//! menu click, needs JNI symbols named after the *app's* package, so those exports
//! live in the app crate, not here.
//!
//! This crate deliberately does not invent a lifecycle model. Pause, resume and
//! destroy become events, and the docs say plainly that a command can be
//! interrupted by process death.

#![cfg(target_os = "android")]

pub use kiln;
/// Re-exported so an app needs only `kiln-android` in its manifest.
pub use tao;
pub use wry;

use std::sync::Arc;

use kiln::{App, AppHandle, Registry};

/// Return the live runtime, if the app has started.
///
/// Forwarded rather than kept here: `App::run` publishes the registry and handle
/// it actually built, and a second copy would answer with a registry that has no
/// commands in it. One publisher, one answer.
pub fn runtime() -> Option<(&'static Arc<Registry>, &'static AppHandle)> {
    kiln::runtime()
}

/// Start a Kiln app from the Android entry point.
///
/// Call this from the function named in `android_binding!`; it blocks for the
/// lifetime of the activity, exactly like `App::run` on desktop.
///
/// Nothing is published here. `App::run` hands its own registry and handle to
/// `kiln::platform` before the loop starts, and [`runtime`] reads them back; the
/// shell cannot be duplicated by the crate that starts it without the bridge and
/// the webview disagreeing about which commands exist.
pub fn run(app: App) {
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

/// Write a line to logcat via the platform logger.
///
/// `liblog` is part of the NDK, so this needs no extra crate. Kept private and
/// tiny: it exists so Android bootstrap diagnostics are visible at the point where
/// stdout is not yet wired up.
pub fn log(message: &str) {
    __android_log(message);
}

fn __android_log(message: &str) {
    extern "C" {
        fn __android_log_write(prio: i32, tag: *const u8, text: *const u8) -> i32;
    }
    const ANDROID_LOG_INFO: i32 = 4;
    let tag = b"Kiln\0";
    let mut text = message.as_bytes().to_vec();
    text.push(0);
    // SAFETY: both pointers are NUL-terminated and outlive the call.
    unsafe {
        __android_log_write(ANDROID_LOG_INFO, tag.as_ptr(), text.as_ptr());
    }
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
    // Route through logcat directly: at this point tao has not yet redirected
    // stdout to the log pipe, so `println!` would vanish.
    __android_log(package);
    // The shell needs its own reference: `ndk_context` only ever holds the
    // *application* context, and a picker, a `PopupMenu`, or a notification can
    // only be started from the activity. Publishing it here, on every create,
    // is what a configuration change needs: the old activity is destroyed, so
    // keeping the first one would mean calling into a dead object.
    kiln::init_activity(activity.clone());
    // SAFETY: this is the hook wry documents for exactly this call, on the thread
    // where the looper is local. tao passes the package name it stored from the
    // macro's `$domain`/`$package`, which is what wry's Kotlin classes are keyed on.
    unsafe { ::wry::android_setup(package, env, looper, activity) }
}
