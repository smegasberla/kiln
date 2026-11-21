//! The commands the shell registers for itself.
//!
//! They share the registry, the allowlist and the envelope with an app's own
//! commands, so a page cannot tell which of the two answered it, and an allowlist
//! withholds a builtin exactly like anything else.

use serde_json::Value;

use crate::{Config, Registry};

// Only the tray and menu commands can fail; everything else answers
// unconditionally, so the error type is unused in a minimal build.
#[cfg(any(feature = "tray", feature = "menu"))]
use crate::Error;

#[cfg(feature = "dialogs")]
use crate::dialogs;
#[cfg(feature = "storage")]
use crate::storage;
#[cfg(feature = "updater")]
use crate::updater;
use crate::WindowOptions;
use crate::{diagnostics, platform, reporting};

/// Register the commands the shell provides itself.
pub(crate) fn register_builtins(registry: &mut Registry, config: &Config) {
    registry.add("__voussoir_version", {
        let name = config.name.clone();
        move |_app, _payload| {
            Ok(serde_json::json!({
                "app": name,
                "voussoir": env!("CARGO_PKG_VERSION"),
            }))
        }
    });

    registry.add("__voussoir_reports", |_app, _payload| {
        Ok(serde_json::json!(reporting::entries()))
    });

    // Dev diagnostics: what `voussoir dev --verbose` prints, in the form a page can
    // render. Answers even when verbose is off, because the buffer is what
    // records the last few reloads and denials, not a live-tailing feature.
    registry.add("__voussoir_diagnostics", |_app, payload| {
        let keep = payload.get("tail").and_then(Value::as_u64).unwrap_or(0) as usize;
        let enabled = diagnostics::enabled();
        let entries = match keep {
            0 => diagnostics::entries(),
            keep => diagnostics::tail(keep),
        };
        Ok(serde_json::json!({ "verbose": enabled, "entries": entries }))
    });

    registry.add("__voussoir_diagnostics_enabled", |_app, _payload| {
        Ok(serde_json::json!({ "verbose": diagnostics::enabled() }))
    });

    // A page that writes user data needs somewhere to put it, and the answer
    // differs per OS. Answered here rather than in the shim so the page never
    // has to guess a platform. `null` means it could not be determined, which
    // the page should treat as "no persistence available".
    registry.add("__voussoir_data_dir", {
        let name = config.name.clone();
        move |_app, _payload| {
            let path = platform::data_dir(&name).and_then(|dir| dir.to_str().map(str::to_owned));
            Ok(serde_json::json!({ "path": path }))
        }
    });

    // Returns the label so the page can address the window it just opened. A
    // page that opens two unnamed windows gets `index.html` and `index.html-2`
    // rather than a silent duplicate and an unaddressable pair.
    registry.add("__voussoir_open_window", |app, payload| {
        let label = app.open_window(WindowOptions::from_json(&payload))?;
        Ok(serde_json::json!({ "label": label }))
    });

    // The open labels, in the order the windows opened. Without this a page has
    // to remember every label it ever asked for, and a window closed from the
    // OS side leaves it emitting into nothing.
    registry.add("__voussoir_windows", |app, _payload| {
        Ok(serde_json::json!({ "windows": app.windows() }))
    });

    registry.add("__voussoir_close_window", |app, payload| {
        if let Some(label) = payload.get("label").and_then(Value::as_str) {
            app.close_window(label);
        }
        Ok(Value::Null)
    });

    registry.add("__voussoir_devtools", |app, _payload| {
        app.open_devtools();
        Ok(Value::Null)
    });

    // Runtime tray and menu updates, so an app does not have to re-register
    // anything to change a tick or a tooltip. Each one validates before queueing,
    // so a page gets a real error envelope naming the reason.
    #[cfg(feature = "tray")]
    registry.add("__voussoir_tray_tooltip", |app, payload| {
        let text = payload
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Payload("tray_tooltip needs a \"text\" string".to_owned()))?;
        app.set_tray_tooltip(text)?;
        Ok(Value::Null)
    });

    #[cfg(feature = "tray")]
    registry.add("__voussoir_tray_checked", |app, payload| {
        let id = payload
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Payload("tray_checked needs an \"id\" string".to_owned()))?;
        let checked = payload
            .get("checked")
            .and_then(Value::as_bool)
            .ok_or_else(|| Error::Payload("tray_checked needs a \"checked\" boolean".to_owned()))?;
        app.set_tray_checked(id, checked)?;
        Ok(Value::Null)
    });

    #[cfg(feature = "tray")]
    registry.add("__voussoir_tray_popup", |app, _payload| {
        app.popup_tray()?;
        Ok(Value::Null)
    });

    #[cfg(feature = "menu")]
    registry.add("__voussoir_menu_checked", |app, payload| {
        let id = payload
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Payload("menu_checked needs an \"id\" string".to_owned()))?;
        let checked = payload
            .get("checked")
            .and_then(Value::as_bool)
            .ok_or_else(|| Error::Payload("menu_checked needs a \"checked\" boolean".to_owned()))?;
        app.set_menu_checked(id, checked)?;
        Ok(Value::Null)
    });

    // Android has no menu bar, so the menu is a `PopupMenu` raised on demand.
    // The page owns the button that raises it, because Android expects app chrome
    // (an overflow button or drawer) rather than OS chrome.
    #[cfg(all(feature = "menu", target_os = "android"))]
    registry.add("__voussoir_show_menu", |_app, _payload| {
        if platform::show_menu() {
            Ok(Value::Null)
        } else {
            Err(Error::Handler("no menu is installed to show".to_owned()))
        }
    });

    #[cfg(feature = "storage")]
    storage::register(registry, config);

    #[cfg(feature = "dialogs")]
    dialogs::register(registry);

    #[cfg(feature = "updater")]
    updater::register(registry, config);
}
