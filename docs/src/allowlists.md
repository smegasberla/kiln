# Per-window allowlists

A window that renders untrusted content should not be able to call the whole app.
Voussoir lets you narrow one window's command surface, at startup or when the window is
opened, and the narrowing can only ever remove.

## In `voussoir.json`

```json
{
  "commands": ["greet", "save", "delete"],
  "windows": { "preview": { "commands": ["greet"] } }
}
```

A window labelled `preview` can call `greet` and nothing else. The intersection
rule is the important part: the effective list is the window's list **and** the
global list. A `windows` entry can therefore never grant a command the app
withheld, which means a config file cannot be used to escalate.

## At runtime

```rust
let label = app.open_window(WindowOptions {
    title: Some("Preview".to_owned()),
    allowlist: Some(vec!["greet".to_owned()]),
    ..Default::default()
})?;
```

The narrowing is registered before the window exists, so its page can never make a
call it will not be allowed to keep making. From the page, the same thing:

```ts
await window.voussoir.invoke("__voussoir_open_window", {
  label: "preview",
  title: "Preview",
  allowlist: ["greet"],
});
```

The built-in answers `{"label": "preview"}`, so the page can address the window it
just opened.

## Where the label comes from

The shell reads the calling window's label from the webview the request arrived
on, never from the payload. A page cannot claim another window's permissions by
sending a label of its own; there is no field for one.

## What a denial looks like

`kind: "unknown"`, the same answer an unregistered command gets. A narrowed window
cannot probe for the commands it is missing, which is the point: a preview window
that can distinguish "not allowed" from "does not exist" has learned part of the
app's surface.

## Proof

`bun scripts/verify.ts` builds a window with a strict list, calls out of it, and
asserts the denial arrives over the real transport on Linux, Windows and macOS. The
`ipc` unit tests cover the intersection rule and the per-window lookup directly.
