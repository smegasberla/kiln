//! Android entry point for Scratch.
//!
//! The counterpart of `native/src/main.rs`: same app, same commands. The only
//! difference is that the activity owns the event loop instead of `main`.
//!
//! `register_jni!` installs the symbols wry's generated Kotlin calls. It must run
//! inside a function because tao's macro expands to a block, which Rust rejects at
//! module scope; `#[no_mangle]` still exports from within a function.

mod assets;

// The app lives in the desktop crate and is included here rather than copied, so
// the two shells cannot disagree about which commands exist. `voussoir android build`
// checks that too, but a check that can pass while the lists differ is worth less
// than a single list that cannot.
#[path = "../../native/src/app.rs"]
mod app;

/// The activity's native entry point, named as JNI expects.
#[no_mangle]
pub extern "C" fn Java_com_voussoir_scratch_MainActivity_android_1main(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    android_main();
}

fn android_main() {
    voussoir_android::register_jni!(com_voussoir, scratch, Rust, android_main);

    // The same `App` the desktop binary runs, so every command exists on both.
    voussoir_android::run(app::build("Scratch"))
}

// Java -> Rust. `MainActivity` calls these when a picker returns, when a popup
// menu item is tapped, and when a notification action is tapped. Their names are
// the activity's method names behind JNI's `Java_<package>_<class>_` prefix, and
// none of those method names contains an underscore, so no `_1` escaping shows
// up here; the package half of the name is already escaped by `com_voussoir_scratch`.
//
// They live in the app rather than in `voussoir-android` because the prefix names
// the app's own package, which the library cannot know.

/// A Java string as Rust text, or `None` when it was null or unreadable.
///
/// An unreadable id is dropped rather than turned into `""`: an empty id names
/// no menu item, so letting one through would emit a click on nothing.
fn text_of(env: &mut jni::JNIEnv, value: &jni::objects::JString) -> Option<String> {
    if value.is_null() {
        return None;
    }
    env.get_string(value).ok().map(String::from)
}

/// The picker answered. `json` is the desktop dialogs' reply shape, or null for
/// a cancel, which `voussoirCompleteDialog` passes through as such.
#[no_mangle]
pub extern "C" fn Java_com_voussoir_scratch_MainActivity_voussoirCompleteDialog(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    id: jni::sys::jlong,
    json: jni::objects::JString,
) {
    voussoir::complete_dialog(id as u64, text_of(&mut env, &json));
}

/// The picker could not be shown at all, which is not the same as a cancel.
#[no_mangle]
pub extern "C" fn Java_com_voussoir_scratch_MainActivity_voussoirFailDialog(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    id: jni::sys::jlong,
    message: jni::objects::JString,
) {
    let message = text_of(&mut env, &message)
        .unwrap_or_else(|| "the file picker could not be shown".to_owned());
    voussoir::fail_dialog(id as u64, &message);
}

#[no_mangle]
pub extern "C" fn Java_com_voussoir_scratch_MainActivity_voussoirMenuClicked(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    id: jni::objects::JString,
) {
    if let Some(id) = text_of(&mut env, &id) {
        voussoir::menu_clicked(&id);
    }
}

/// A lifecycle transition the activity reported: `resume`, `pause`, `stop`, or
/// `destroy`.
///
/// Queued rather than answered, and never dropped. tao starts the event loop on
/// its own thread, so the first `resume` arrives before `App::run` has published
/// a handle and before the page can run a script; `voussoir::lifecycle` keeps the
/// states until the page has loaded.
#[no_mangle]
pub extern "C" fn Java_com_voussoir_scratch_MainActivity_voussoirLifecycle(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    state: jni::objects::JString,
) {
    if let Some(state) = text_of(&mut env, &state) {
        voussoir::lifecycle(&state);
    }
}

/// True when the click reached the shell.
///
/// A notification can start the process, and the tap then resumes the activity
/// before `App::run` has published its handle; `tray_clicked` drops an event it
/// has no loop to send to, so the answer here is whether that loop exists. The
/// activity keeps the intent extra and tries again on the next resume.
#[no_mangle]
pub extern "C" fn Java_com_voussoir_scratch_MainActivity_voussoirTrayClicked(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    id: jni::objects::JString,
) -> jni::sys::jboolean {
    let Some(id) = text_of(&mut env, &id) else {
        return jni::sys::JNI_FALSE;
    };
    if voussoir::runtime().is_none() {
        return jni::sys::JNI_FALSE;
    }
    voussoir::tray_clicked(&id);
    jni::sys::JNI_TRUE
}
