# Multi-window

A window is a label, a page, and a command surface. Everything else (sending an
event, closing it, narrowing it) addresses a window by that label.

## Opening one

```rust
let label = app.open_window(WindowOptions {
    title: Some("Settings".to_owned()),
    width: Some(420.0),
    height: Some(320.0),
    entry: None,          // the app's entry page
    allowlist: None,      // the app's own surface
    ..Default::default()
})?;

// The window does not exist yet: creation happens on the UI thread. The label is
// already reserved, so this is safe to send now and delivered on `on_ready`.
app.emit_to(Some(&label), "settings:open", serde_json::json!(null));
```

`open_window` returns the resolved label rather than `()`. Two reasons, both
practical:

- A handler that opens a window and then wants to talk to it needs the name
  *now*, and before this returned nothing it had to duplicate the naming rule.
- An unnamed window must not collide with another unnamed window. The shell
  generates `index.html`, `index.html-2`, and so on, and hands you the result.

From the page, `__kiln_open_window` answers `{"label": "..."}` for the same
reason.

## Listing and closing

```rust
app.windows();              // ["main", "settings"], in opening order
app.close_window("settings");
```

```ts
await window.kiln.windows();                    // same list
await window.kiln.invoke("__kiln_close_window", { label: "settings" });
```

The list is maintained by the shell, not by the page, so it stays true when a
window is closed by the OS (the title bar) rather than by your code. A page that
tracks labels itself drifts the first time a user closes a window the app did not
close.

## Addressing one window

```rust
app.emit_to(Some("settings"), "theme", json!({ "dark": true }));
app.emit_to(None, "broadcast", json!({ "save": true }));   // every window
```

`emit_to(None, …)` is the broadcast form; `emit_to(Some(label), …)` reaches exactly
one window. Both go through the event loop proxy, so they are safe from any thread
and are queued rather than synchronous.

## Per-window narrowing

A window opened for untrusted content gets a smaller surface:

```rust
let preview = app.open_window(WindowOptions {
    title: Some("Preview".to_owned()),
    allowlist: Some(vec!["greet".to_owned()]),
    ..Default::default()
})?;
```

See [Per-window allowlists](allowlists.md) for the intersection rule and what a
denial looks like.

## Closing the last window

When the last window closes, the app exits. If a window must survive (a tray app,
a background sync), declare it on the tray config with `keep_running` so the
event loop does not exit on the last close.

## Next

- [Lifecycle hooks](lifecycle.md) — the first and last safe moment for each window.
