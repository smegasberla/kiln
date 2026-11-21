# Multi-window

A window is a label, a page, and a command surface. Everything else (sending an
event, closing it, narrowing it) addresses a window by that label.

## Theory: why labels, and why the shell owns the list

A window needs a name that code can use after the fact: to send it an event, to
close it, to know whether it is still open. A pointer or an index would not work,
because both change when a window closes and another opens.

A **label** is a stable string that survives all of that, and it is also the key
the allowlist uses. So the label does double duty: identity and permission scope.

The list of open windows is maintained by the shell, not the page. A page that
tracks labels itself drifts the first time a user closes a window with the title
bar, because nothing informed the page. Ask `windows()` instead.

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

// The window does not exist yet: creation happens on the UI thread. Do not emit
// to it here. Emit from `on_ready`, which is the first moment the page can
// receive anything.
app.emit_to(Some(&label), "settings:open", serde_json::json!(null)); // DROPPED
```

`open_window` returns the resolved label rather than `()`. Two reasons, both
practical:

- A handler that opens a window and then wants to talk to it needs the name
  *now*, and before this returned nothing it had to duplicate the naming rule.
- An unnamed window must not collide with another unnamed window. The shell
  generates `index.html`, `index.html-2`, and so on, and hands you the result.

From the page, `__voussoir_open_window` answers `{"label": "..."}` for the same
reason.

**The timing rule is worth internalising, because getting it wrong is silent.**
The label exists immediately and is already listed by `windows()`, which counts a
window from the moment it is *requested*. The window itself, and its page, do not
exist yet.

An `emit` is delivered by evaluating a script in each matching webview as the
event is processed, and the shim's hook is a no-op guard (`window.__voussoir_emit&&`).
So an event emitted before the page has installed its listener is **dropped, with
no error**. There is no queue behind it.

That is why `on_ready` exists and why this is the one ordering rule in the API:

```rust
// Wrong: emitted during setup, before the page exists. Vanishes.
app.emit_to(Some("settings"), "settings:open", json!(null));

// Right: emitted from the hook that fires once the page can listen.
App::new().on_ready(|app, label| {
    if label == "settings" {
        app.emit_to(Some(label), "settings:open", json!(null));
    }
})
```

## Listing and closing

```rust
app.windows();              // ["main", "settings"], in opening order
app.close_window("settings");
```

```ts
await window.voussoir.windows();                    // same list
await window.voussoir.invoke("__voussoir_close_window", { label: "settings" });
```

Closing is not asking. There is no `before-close` veto in the reference app; if
you need to prompt for unsaved work, do it in the page before calling
`__voussoir_close_window`, and use `on_window_close` for what must be saved
regardless. See [Lifecycle hooks](lifecycle.md).

## Addressing one window

```rust
app.emit_to(Some("settings"), "theme", json!({ "dark": true }));
app.emit_to(None, "broadcast", json!({ "save": true }));   // every window
```

`emit_to(None, …)` is the broadcast form; `emit_to(Some(label), …)` reaches
exactly one window. Both go through the event loop proxy, so they are safe from
any thread and are queued rather than synchronous.

`emit(...)` is the same as `emit_to(None, ...)`, kept as the shorter spelling for
the common case.

## Theory: why there is no window handle

Some frameworks hand you a window object with a hundred methods. Voussoir
deliberately does not, and the reason is that the handle would have to be `Send`
and usable from a worker thread, which means every method is a message to the UI
thread anyway. A label *is* that message, and it composes: it can be put in a
config, compared, logged and stored.

The cost is that every operation is a command or a method that takes a label,
which is slightly more verbose. The benefit is that state and identity never
disagree.

## Per-window narrowing

A window opened for untrusted content gets a smaller surface:

```rust
let preview = app.open_window(WindowOptions {
    title: Some("Preview".to_owned()),
    allowlist: Some(vec!["greet".to_owned()]),
    ..Default::default()
})?;
```

The narrowing is applied before the window loads its page, and it intersects with
the global allowlist. See
[Per-window allowlists](allowlists.md) for the intersection rule and what a
denial looks like.

## Closing the last window

When the last window closes, the app exits. If a window must survive (a tray app,
a background sync), declare `keep_running()` on the tray config:

```rust
App::new().tray(TrayConfig::new("Notes").keep_running())
```

That is the only switch. There is no "hide to tray on close" default, because an
app that keeps running after the user closed it, without saying so, is how users
learn to distrust close buttons.

## A worked pattern: settings window

```rust
// A command that opens the settings window once and focuses it next time.
fn open_settings(app: &AppHandle) -> Result<Value, voussoir::Error> {
    for label in app.windows() {
        if label == "settings" {
            app.emit_to(Some("settings"), "settings:focus", json!(null));
            return Ok(json!({ "label": label, "reused": true }));
        }
    }
    let label = app.open_window(WindowOptions {
        title: Some("Settings".to_owned()),
        width: Some(420.0),
        height: Some(320.0),
        ..Default::default()
    })?;
    Ok(json!({ "label": label, "reused": false }))
}
```

The page handles `settings:focus` by focusing its first field. This is the whole
multi-window toolkit in one function: list, address, open, and a page-side event.

## Next

- [Lifecycle hooks](lifecycle.md) — `on_ready` and `on_window_close`.
- [Per-window allowlists](allowlists.md) — narrowing a window's surface.
