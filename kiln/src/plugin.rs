//! Plugins: one crate, one call, no manifest.
//!
//! There is deliberately no plugin *system* here. A plugin is an ordinary crate
//! that depends on `kiln` and implements [`Plugin`], and installing it is one
//! builder call:
//!
//! ```ignore
//! use kiln::{App, Plugin};
//!
//! struct Sqlite;
//!
//! impl Plugin for Sqlite {
//!     fn name(&self) -> &'static str { "sqlite" }
//!
//!     fn install(self, app: App) -> App {
//!         app.command("sqlite.query", |_app, payload| {
//!             // ...and nothing else. No registration macro, no capability file.
//!             Ok(payload)
//!         })
//!     }
//! }
//!
//! App::new().plugin(Sqlite).run()
//! ```
//!
//! That is the whole contract: whatever `install` returns is the app the next
//! step sees, so a plugin composes with everything the builder already offers,
//! including other plugins.
//!
//! # What this is not
//!
//! - **Not a sandbox.** A plugin is Rust compiled into the app, so it can do what
//!   the app can do. Isolation would mean a separate process or a wasm runtime,
//!   and neither is free; if a dependency cannot be trusted with the app's
//!   authority, it should not be linked in at all.
//! - **Not a manifest.** There is no version negotiation, no JSON table of
//!   exports, and no discovery at runtime. Cargo already resolves versions, and
//!   the linker already fails when a crate is missing.
//! - **Not namespaced for you.** Command names are global, so two plugins that
//!   both claim `open` will silently replace each other, last one winning.
//!   Prefix with the plugin name (`sqlite.query`, `dialog.open`) and use
//!   [`App::has_command`] when a plugin should defer to an existing handler
//!   instead of taking the name.

use crate::App;

/// Something that contributes to an app: commands, a menu, a tray icon, or none
/// of those.
///
/// Implemented by hand rather than derived, because a plugin is usually a type
/// that also holds configuration:
///
/// ```ignore
/// pub struct Sqlite { pub path: std::path::PathBuf }
///
/// impl Plugin for Sqlite {
///     fn name(&self) -> &'static str { "sqlite" }
///     fn install(self, app: App) -> App {
///         let path = self.path;
///         app.command("sqlite.query", move |_app, payload| {
///             let _ = &path;
///             Ok(payload)
///         })
///     }
/// }
/// ```
pub trait Plugin {
    /// A short, stable identifier: `"sqlite"`, `"dialog"`. Used in error
    /// messages and diagnostics, not for lookup.
    fn name(&self) -> &'static str;

    /// Add whatever this plugin provides and hand the app back.
    ///
    /// Returning the app rather than mutating it in place is what lets the
    /// builder chain, and it means a plugin cannot be called twice by accident
    /// on the same app.
    fn install(self, app: App) -> App
    where
        Self: Sized;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use serde_json::Value;

    struct Greets;

    impl Plugin for Greets {
        fn name(&self) -> &'static str {
            "greets"
        }

        fn install(self, app: App) -> App {
            app.command("greets.hello", |_app, _payload| {
                Ok(Value::String("hi".to_owned()))
            })
        }
    }

    #[test]
    fn installing_a_plugin_registers_its_commands() {
        let app = App::new().plugin(Greets);
        assert!(app.has_command("greets.hello"));
        assert_eq!(Greets.name(), "greets");
    }

    #[test]
    fn a_plugin_sees_commands_registered_before_it() {
        // The reason `has_command` exists: a plugin that defers to the app's own
        // handler instead of silently replacing it.
        struct Optional;

        impl Plugin for Optional {
            fn name(&self) -> &'static str {
                "optional"
            }

            fn install(self, app: App) -> App {
                if app.has_command("optional.thing") {
                    return app;
                }
                app.command("optional.thing", |_app, _payload| {
                    Err(Error::Handler("not configured".to_owned()))
                })
            }
        }

        let app = App::new()
            .command("optional.thing", |_app, _payload| Ok(Value::Null))
            .plugin(Optional);
        assert!(app.has_command("optional.thing"));
    }
}
