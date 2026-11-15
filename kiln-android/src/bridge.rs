//! The JNI bridge that replaces the custom-protocol transport on Android.
//!
//! Android's WebView does not make a custom URL scheme pleasant to intercept, so
//! the page calls into Kotlin/Java through `@JavascriptInterface`, and that class
//! calls the exported JNI function below. The request and the reply use the same
//! envelope as the desktop transport, so a command behaves identically on both.
//!
//! Kotlin side (generated into a scaffolded project):
//!
//! ```kotlin
//! class KilnBridge {
//!   external fun invoke(method: String, payload: String): String
//! }
//! ```
//!
//! The page then sees `kiln.invoke(...)` exactly as on desktop.

use jni::{
    objects::{JClass, JString},
    sys::jstring,
    JNIEnv,
};

/// `Java_<package>_KilnBridge_invoke`
///
/// Kept as a plain `extern "C"` export rather than going through `android_fn!`
/// because the bridge is a leaf: it needs no activity reference, and this way the
/// symbol name is visible and greppable.
#[no_mangle]
pub extern "C" fn Java_com_kiln_android_KilnBridge_invoke<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    method: JString<'local>,
    payload: JString<'local>,
) -> jstring {
    let method: String = match env.get_string(&method) {
        Ok(value) => value.into(),
        Err(_) => return error_string(&mut env, "kiln: bridge could not read the method"),
    };
    let payload: String = match env.get_string(&payload) {
        Ok(value) => value.into(),
        Err(_) => return error_string(&mut env, "kiln: bridge could not read the payload"),
    };

    let response = crate::invoke(&method, &payload);
    match env.new_string(response) {
        Ok(value) => value.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Answers with an error envelope rather than a null pointer, so the page always
/// gets JSON it can parse.
fn error_string(env: &mut JNIEnv, message: &str) -> jstring {
    let envelope = format!(
        r#"{{"error":{}}}"#,
        kiln::serde_json::Value::from(message)
    );
    env.new_string(envelope)
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_envelope_shape_matches_the_desktop_transport() {
        // The bridge must not be able to invent a different reply shape.
        let ok = format!(r#"{{"ok":{}}}"#, 1);
        assert_eq!(ok, r#"{"ok":1}"#);

        let err = format!(r#"{{"error":{}}}"#, kiln::serde_json::Value::from("boom"));
        assert_eq!(err, r#"{"error":"boom"}"#);
    }
}
