# Per-window allowlists

A window that renders untrusted content should not be able to call the whole app.
Voussoir lets you narrow one window's command surface, at startup or when the
window is opened, and the narrowing can only ever remove.

## Theory: the confused-deputy problem

If your app opens a window showing someone else's content, that content runs in a
window that can call your commands. It cannot read your files itself, so it asks
the window for what it wants, and the window obliges. The content has used your
app's authority without ever holding it.

This is the classic confused deputy. The defence is not to harden the content;
it is to make sure the deputy has less authority when doing someone else's
bidding. In an IPC-shaped app, that means **the command surface is per window**.

The alternative design, a single global surface, cannot express this, and that is
why the allowlist is per window rather than a global list only.

## In `voussoir.json`

```json
{
  "commands": ["greet", "save", "delete"],
  "windows": { "preview": { "commands": ["greet"] } }
}
```

A window labelled `preview` can call `greet` and nothing else.

**The intersection rule is the important part:** the effective list is the
window's list **and** the global list. A `windows` entry can therefore never grant
a command the app withheld, which means a config file cannot be used to escalate.

```
effective(preview) = global ∩ windows.preview
                   = {greet, save, delete} ∩ {greet}
                   = {greet}
```

Add `"delete"` to `preview` and it is still not grantable, because the
intersection is what applies.

## At runtime

```rust
let label = app.open_window(WindowOptions {
    title: Some("Preview".to_owned()),
    allowlist: Some(vec!["greet".to_owned()]),
    ..Default::default()
})?;
```

The narrowing is registered **before the window exists**, so its page can never
make a call it will not be allowed to keep making. That ordering is the point: a
window that is briefly wide open and then narrowed would have a window of
opportunity, and a page loads fast.

From the page, the same thing:

```ts
const { label } = await window.voussoir.invoke<{ label: string }>(
  "__voussoir_open_window",
  { label: "preview", title: "Preview", allowlist: ["greet"] },
);
```

The built-in answers `{"label": "preview"}`, so the page can address the window it
just opened even if the shell had to generate a unique label.

## Where the label comes from

The shell reads the calling window's label from the webview the request arrived
on, **never from the payload**. A page cannot claim another window's permissions
because there is no field for one.

This is worth pausing on, because it is the one property that a page cannot
influence. If the label came from the payload, a narrowed window would simply
send `{"window": "main"}` and the whole mechanism would be decoration.

## Theory: what a denial looks like, and why it looks wrong

`kind: "unknown"`, the same answer an unregistered command gets.

A narrowed window cannot probe for the commands it is missing, which is the point:
a preview window that can distinguish "not allowed" from "does not exist" has
learned part of the app's surface.

The cost is real and worth knowing: during development, a typo in an allowlist
looks exactly like a typo in a command name. Both answer `unknown`.

The crate narrows that gap by auditing the allowlist automatically at startup: it
checks every name in `commands` and in every `windows` entry against the registry,
and logs the ones no handler can answer. You do not call it; it runs as part of
`App::run`. A config that names a command you renamed therefore says so on stderr
instead of looking like a missing command.

It cannot catch the other case, a command that exists and is withheld, because
that is the mechanism working.

## Patterns

**A preview window** — the motivating case:

```json
{
  "commands": ["greet", "save", "preview.render"],
  "windows": { "preview": { "commands": ["preview.render"] } }
}
```

`preview.render` returns HTML and cannot touch the disk; `save` stays in the main
window.

**A plugin's commands per window** — narrowed without the plugin cooperating:

```json
{ "windows": { "settings": { "commands": ["sqlite.query", "storage.get"] } } }
```

**An empty list** — a window with no surface at all, which is what you want for
anything purely decorative or fully untrusted:

```json
{ "windows": { "help": { "commands": [] } } }
```

Note the difference from omitting the key: absent means "inherit the global
list"; `[]` means "nothing".

## Testing it

The `ipc` unit tests cover the intersection rule and the per-window lookup
directly, and `bun scripts/verify.ts` builds a window with a strict list, calls
out of it, and asserts the denial arrives over the real transport.

A useful assertion for an app is the envelope a denied call produces, because
that is what the page will see. The crate's own `ipc` tests do it this way:

```rust
#[test]
fn preview_window_cannot_reach_delete() {
    let mut registry = Registry::new();
    registry.set_allowlist(Some(vec!["greet".to_owned(), "delete".to_owned()]));
    registry.set_window_allowlist(HashMap::from([(
        "preview".to_owned(),
        vec!["greet".to_owned()],
    )]));
    // ... register the handlers, then dispatch as "preview":
    assert!(dispatch(&registry, "preview", "greet").contains(r#""ok""#));
    assert!(dispatch(&registry, "preview", "delete").contains(r#""kind":"unknown""#));
}
```

`Registry` has no `allows(label, name)` predicate: the intersection is applied
inside dispatch, so a denial is observable as `kind: "unknown"` rather than as a
boolean. Assert the envelope, which is also what the page branches on.

## Next

- [Multi-window](multi-window.md) — labels, listing and closing.
- [Security model](security.md) — the rest of the policy.
