# Lifecycle hooks

Two hooks bracket every window, including ones opened long after startup. Both
fire exactly once per window, guarded against duplicate platform events, so "once"
is a property of the code rather than of the webview's event habits.

## `on_ready`

```rust
App::new().on_ready(|app, label| {
    // The first moment `emit_to` is safe for this window.
    app.emit_to(Some(label), "boot", json!({ "version": env!("CARGO_PKG_VERSION") }));
})
```

`on_ready` fires after the page has loaded and the shim is installed, which is the
earliest point an event can reach a listener. It fires once per window, in
whatever order the windows finish loading; a window opened from a command is
covered as long as the hook was registered before the app started.

Use it for the state a window needs at birth: the current document, the theme, the
user's preferences. Do not use it to poll: the page can ask for state itself with
`invoke`, and a hook that emits on a timer is a hook that eventually races the
window closing.

## `on_window_close`

```rust
App::new().on_window_close(|_app, label| {
    // Per-window state should be persisted here, not in a Drop impl.
    if let Some(draft) = kiln::state::get(&format!("draft:{label}")) {
        kiln::state::set(&format!("saved:{label}"), draft);
    }
})
```

Fires once as a window goes away, whichever side closed it. This is where per-window
state belongs: a `Drop` impl on your own struct runs whenever Rust drops it, which
is not the same moment, and a struct moved into the handler closure is dropped at
process exit rather than at window close.

The hook runs before the window is removed from the open list, so
`app.windows()` inside it still contains the closing window. That is intentional:
persisting "the set of windows" is a thing apps do, and it should include the one
being closed.

## Ordering guarantees

For one window: `on_ready` strictly before any event it emits is delivered, and
`on_window_close` strictly after the last event it can receive. Across windows
there is no ordering, which is why both hooks take a label.

The first window is a special case on iOS: `tao` cannot build a window before
`UIApplicationMain` runs, so the first window is requested through the same path a
command uses and `on_ready` fires once the loop is up. The hook contract is the
same; only the timing differs.

## Why not window events

Listening to raw window events would give you `Resized`, `Focused`, `Moved` and
much more. Kiln does not expose them, because an app that needs to know about focus
usually needs to know about *content* focus, and that belongs in the page. The two
hooks here are the ones with a correctness requirement: something must happen at
the start, and something must be saved at the end.
