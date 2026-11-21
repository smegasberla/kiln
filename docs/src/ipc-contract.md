# Commands & payloads

Every call between the page and Rust goes through one envelope, in both
directions, over one endpoint. The point of a single shape is that a page can
report failures, and the shell can report failures, without either side inventing
a second convention.

```
POST kiln://localhost/__kiln/<method>
body:     <payload as JSON, or null>
response: {"ok": <json>}
        | {"error": {"message": "<text>", "kind": "<class>", "method": "<name>"}}
```

`kind` is one of:

| `kind` | Meaning |
| --- | --- |
| `unknown` | no handler, or one withheld by an allowlist. Deliberately the same answer for both |
| `payload` | the request did not match the handler's argument type |
| `handler` | the handler returned `Err` |
| `panic` | the handler panicked; the app kept running and the page was told |
| `runtime` | the webview or event loop failed |

`method` names the command that failed, even when the call came from a nested
helper. Branch on `kind`; never match `message` text, which is for humans.

## The page side

The shim installs `window.kiln` before any of your scripts run:

```ts
const greeting = await window.kiln.invoke<string>("greet", { name: "you" });

const off = window.kiln.listen<{ value: number }>("count", (event) => {
  render(event.value);
});

// Later, or when the component unmounts:
off();
```

`invoke` rejects with an error carrying the `kind`, so a `catch` can tell a missing
command from a bad payload. `listen` returns an unsubscribe function; calling it
twice is harmless.

The shim also carries `platform` (`"linux"`, `"windows"`, `"macos"`, `"bsd"`,
`"android"`, `"ios"`) and `protocol`, the shim's own version number. Feature-detect
on `protocol`, not on `platform`: a page that needs `windows()` asks
`kiln.protocol >= 2` and degrades in one place instead of in every platform branch.

## The Rust side

A command that needs no app handle lives in the commands crate, in one table:

```rust
use kiln::{Command, Kind};
use kiln::serde_json::{json, Value};

pub static COMMANDS: &[Command] = &[
    Command { name: "greet", kind: Kind::Sync, run: greet },
];

fn greet(payload: &Value) -> Result<Value, String> {
    let name = payload.get("name").and_then(Value::as_str).unwrap_or("world");
    Ok(json!(format!("Hello, {name}!")))
}
```

A command that needs the app itself is registered on `App` in `main.rs`, because a
handle cannot cross the `cdylib` boundary soundly:

```rust
App::new()
    .command("open_settings", |app, _payload| {
        app.open_window(WindowOptions {
            label: Some("settings".to_owned()),
            title: Some("Settings".to_owned()),
            ..Default::default()
        })?;
        Ok(Value::Null)
    })
    .run()
```

Both kinds are reachable from the page with the same `invoke`, gated by the same
allowlist, and answered with the same envelope.

## Sync, worker, async

There are three execution models, and picking the right one is the only
performance decision the contract asks you to make.

```rust
// Runs on the UI thread. Cheap work only: it blocks painting.
.command("greet", |_app, payload| Ok(payload))

// Runs on its own worker thread. For blocking I/O the webview must not wait on.
.async_command("read_file", |_app, _payload| {
    std::fs::read_to_string("/etc/hostname")
        .map(|text| json!(text))
        .map_err(kiln::Error::Handler)
})

// A real async fn on the shared runtime. Requires the `rt` feature.
.async_fn("fetch", |_app, _payload: FetchArgs| async move {
    let body = kiln::rt::http_get("https://example.com").await?;
    Ok(json!(body))
})
```

`async_command` takes a `Value` and spawns a thread per call; `async_fn` takes a
typed argument and submits a task to one shared multi-threaded runtime. For a
handful of concurrent commands the difference is noise, and
`bun scripts/bench.ts --dispatch` measures it rather than guessing. Use `async_fn`
when the handler awaits; use `async_command` when it merely blocks.

## Pushing the other way

```rust
app.emit_to(Some("main"), "document:changed", json!({ "dirty": true }));
app.emit_to_all("theme", json!({ "dark": true }));
```

Emitting goes through the event loop proxy and `evaluate_script`, so it is
thread-safe and never touches the webview off the UI thread. That also means it is
queued, not synchronous: a handler that emits and immediately returns does not
guarantee the page has run its listener.

## Next

- [Typed commands](typed-commands.md) removes the `Value` marshalling.
- [Errors](errors.md) covers what each failure looks like to the page.
