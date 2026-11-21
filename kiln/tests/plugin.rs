//! The plugin contract as an outside crate sees it.
//!
//! The unit tests in `plugin.rs` are inside the crate, so they can see items a
//! real plugin cannot: a `pub(crate)` builder, a private registry field. This file
//! is compiled as a separate crate against `kiln`'s public API only, which is the
//! only way to know that "a plugin is just a crate that implements `Plugin`" is
//! true and not just true from the inside.

use kiln::{App, Error, Plugin};
use serde_json::{json, Value};

/// A plugin shaped like a real one: holds configuration, registers a command,
/// namespaces its command after itself.
struct Counter {
    start: u64,
}

impl Plugin for Counter {
    fn name(&self) -> &'static str {
        "counter"
    }

    fn install(self, app: App) -> App {
        let start = self.start;
        app.command("counter.next", move |_app, payload| {
            let by = payload.get("by").and_then(Value::as_u64).unwrap_or(1);
            Ok(json!(start + by))
        })
    }
}

#[test]
fn an_outside_crate_can_be_a_plugin() {
    let app = App::new().plugin(Counter { start: 10 });
    assert!(app.has_command("counter.next"));
}

#[test]
fn a_plugin_installed_second_sees_the_first() {
    // Two plugins in a chain: the second must observe what the first registered,
    // which is only possible because `install` hands the app through.
    struct Observer {
        saw: std::sync::Arc<std::sync::Mutex<bool>>,
    }

    impl Plugin for Observer {
        fn name(&self) -> &'static str {
            "observer"
        }

        fn install(self, app: App) -> App {
            let saw = std::sync::Arc::clone(&self.saw);
            *saw.lock().unwrap() = app.has_command("counter.next");
            app
        }
    }

    let saw = std::sync::Arc::new(std::sync::Mutex::new(false));
    App::new()
        .plugin(Counter { start: 0 })
        .plugin(Observer {
            saw: std::sync::Arc::clone(&saw),
        });
    assert!(*saw.lock().unwrap(), "the second plugin could not see the first");
}

#[test]
fn a_plugin_can_defer_to_a_command_the_app_registered() {
    // The hazard of last-wins registration: a plugin that claims a name the app
    // already uses silently replaces the app's handler. `has_command` is the
    // check that makes deferring possible, so this test is about the check
    // rather than the plugin.
    struct Defers;

    impl Plugin for Defers {
        fn name(&self) -> &'static str {
            "defers"
        }

        fn install(self, app: App) -> App {
            if app.has_command("app.greet") {
                return app;
            }
            app.command("app.greet", |_app, _payload| {
                Err(Error::Handler("plugin default".to_owned()))
            })
        }
    }

    let app = App::new()
        .command("app.greet", |_app, _payload| Ok(json!("app")))
        .plugin(Defers);
    assert!(app.has_command("app.greet"));
}

#[test]
fn a_plugin_can_stack_typed_and_async_commands() {
    // A plugin is not limited to `command`; it composes with the whole builder,
    // which is the reason `install` returns `App` instead of taking `&mut`.
    #[derive(serde::Deserialize)]
    struct Args {
        text: String,
    }

    struct AllKinds;

    impl Plugin for AllKinds {
        fn name(&self) -> &'static str {
            "all-kinds"
        }

        fn install(self, app: App) -> App {
            app.command_typed("all.typed", |_app, args: Args| Ok(args.text))
                .async_command("all.worker", |_app, payload| Ok(payload))
        }
    }

    let app = App::new().plugin(AllKinds);
    assert!(app.has_command("all.typed"));
    assert!(app.has_command("all.worker"));
}

#[test]
fn the_trait_is_usable_as_a_bound() {
    // Someone will want `fn add_all<P: Plugin>(app: App, plugins: Vec<P>)`. That
    // only compiles if the trait is public with no sealed-module surprise.
    //
    // The plugins must be distinct types: two `Counter`s both claim
    // `counter.next`, and since registration is strict in a test build that is
    // a panic. It is the collision the check exists for, so the test installs
    // one of each rather than working around it.
    fn install_all(app: App) -> App {
        app.plugin(Counter { start: 1 }).plugin(Marker)
    }

    struct Marker;

    impl Plugin for Marker {
        fn name(&self) -> &'static str {
            "marker"
        }

        fn install(self, app: App) -> App {
            app.command("marker.ping", |_app, _payload| Ok(Value::Null))
        }
    }

    let app = install_all(App::new());
    assert!(app.has_command("counter.next"));
    assert!(app.has_command("marker.ping"));
}
