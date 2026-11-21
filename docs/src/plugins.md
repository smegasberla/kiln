# Plugins

A plugin is a crate that implements one trait. There is no plugin system beyond
that: no manifest, no capability file, no discovery at runtime, and no macro.

```rust
use kiln::{App, Error, Plugin};
use kiln::serde_json::Value;

pub struct Sqlite {
    pub path: std::path::PathBuf,
}

impl Plugin for Sqlite {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    fn install(self, app: App) -> App {
        let path = self.path;
        app.command("sqlite.query", move |_app, payload| {
            let _ = &path;
            // ...real work here
            Ok(payload)
        })
    }
}
```

The app installs it with one call:

```rust
App::new().plugin(Sqlite { path: "app.db".into() }).run()
```

`install` takes the app and returns it, so a plugin composes with the builder and
with other plugins. Order matters only for name collisions.

## Why there is no manifest

Cargo already resolves versions, and the linker already fails when a crate is
missing. A JSON table of exports would add a second source of truth that can
disagree with the code, and a version handshake that the compiler already
performs. The dependency edge *is* the manifest.

## What a plugin cannot do

A plugin is Rust compiled into the app, so it has the app's authority. It can read
the disk, open a socket, and register any command name. If a dependency cannot be
trusted with that, it should not be linked in; isolation would mean a separate
process or a wasm runtime, and neither is free.

## Names are global

Registration is last-wins. Two plugins that both claim `open` will silently
replace each other. Prefix with the plugin name (`sqlite.query`, `dialog.open`),
and use `has_command` when a plugin should defer to an existing handler instead
of hijacking it:

```rust
fn install(self, app: App) -> App {
    if app.has_command("sqlite.query") {
        return app;
    }
    app.command("sqlite.query", /* ... */)
}
```

`has_command` ignores the allowlist: a registered command that a config withholds
is still registered, otherwise the answer would depend on which window asked.

## The allowlist still applies

A plugin's commands share the registry, the envelope and the allowlist with
everything else. Naming them in `kiln.json` keeps them; leaving them out removes
them from every page.
