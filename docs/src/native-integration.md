# Native integration: menu, tray, dialogs

Three features make an app feel native rather than like a page in a box. All
three are cargo features, so an app that does not need them pays nothing for
them, and all three surface to the page as ordinary commands, so the allowlist
applies to them like everything else.

## Theory: why these are features and not defaults

Each one pulls a large platform crate: `muda` for menus, `tray-icon` for the
tray, `rfd` for file dialogs. Linking them into an app that never opens a menu
adds binary size and a set of platform edge cases you did not ask for.

So the rule is: **on if the app uses it, off if it does not.** The scaffold turns
them on (`features = ["full", "storage"]`) because a new app usually wants a
menu; a library-shaped app should narrow that.

| Feature | Adds | Platform gaps |
| --- | --- | --- |
| `menu` | native menu bar, standard edit items | no menu bar on iOS |
| `tray` | tray icon and its context menu | Android posts a notification instead; absent on iOS |
| `dialogs` | `dialog.open`, `dialog.save`, `dialog.folder`, `dialog.open_many` | none of note |
| `devtools` | devtools in release builds, on F12 / Cmd-Alt-I | webview without devtools |

## Menus

A menu is declared once, in Rust, and the items are what the page can address.

```rust
use voussoir::menu::{MenuGroup, MenuItem};

App::new()
    .menu(
        MenuGroup::new("File")
            .item(MenuItem::new("new", "New").accelerator("CmdOrCtrl+N").command("file.new"))
            .item(MenuItem::new("save", "Save").accelerator("CmdOrCtrl+S").command("file.save")),
    )
    .menu(
        MenuGroup::new("View").item(MenuItem::new("dark", "Dark mode").check().checked(false)),
    )
```

`.menu(...)` is called once per group, so groups are added by repeating it rather
than by passing a list.

Three pieces, each with a purpose:

- **`id`** is how you address the item later (`menu.checked("dark", true)`). It is
  never shown.
- **`accelerator`** is the keyboard shortcut, written once in a portable form:
  `CmdOrCtrl+S` becomes ⌘S on macOS and Ctrl+S elsewhere. Do not write `Ctrl`
  unless you mean it on macOS too.
- **`command`** makes activating the item call a command, exactly as if the page
  had invoked it. That is the important part: a menu item is not a second kind of
  handler.

An item with `.check()` is a toggle. Tick it from Rust or from the page:

```ts
await window.voussoir.menu.checked("dark", true);
```

**Why a command and not a callback?** Because a command already has the
allowlist, the error envelope and the diagnostics. A menu that called a bare
closure would be a second path into your app with none of them.

## File dialogs

Dialogs are asynchronous commands, because the user may take as long as they
like:

```ts
const picked = await window.voussoir.invoke<string | null>("dialog.open", {
  filters: [{ name: "Markdown", extensions: ["md", "markdown"] }],
});
if (picked === null) return; // the user cancelled
```

| Command | Payload | Resolves to |
| --- | --- | --- |
| `dialog.open` | `{ filters?, title? }` | one path, or `null` |
| `dialog.open_many` | `{ filters?, title? }` | a list of paths (possibly empty) |
| `dialog.save` | `{ filters?, title?, name? }` | a path, or `null` |
| `dialog.folder` | `{ title? }` | a path, or `null` |

Two habits worth having:

1. **`null` is cancellation, not failure.** A cancel is a normal outcome and
   should not show an error.
2. **Extensions are given without the dot.** `"md"`, not `".md"`; the dot is
   stripped either way, so the two forms behave the same, but one is consistent.

The dialog returns a path and nothing more. Reading the file is a separate
command of your own, which is where the interesting policy lives:

```rust
fn read_text(payload: &Value) -> Result<Value, String> {
    let path = payload.get("path").and_then(Value::as_str).ok_or("missing path")?;
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(serde_json::json!(text))
}
```

Never let the page name a path it did not receive from a dialog unless the
command is genuinely a filesystem command. That is the difference between a file
picker and an arbitrary-read primitive.

## Tray

The tray is a config, not a set of commands:

```rust
use voussoir::tray::{TrayConfig, TrayItem};

App::new().tray(
    TrayConfig::new("Notes")
        .icon(rgba, 32, 32)               // raw RGBA; omit for the default mark
        .item(TrayItem::new("show", "Show").command("window.show"))
        .item(TrayItem::new("quit", "Quit").command("app.quit"))
        .keep_running(),                  // closing the last window does not exit
)
```

`.tray(...)` takes one `TrayConfig`; the items are chained onto it.

`keep_running()` is the one that changes behaviour rather than appearance: with
it, the app survives having no windows, which is what a tray app wants. Without
it, closing the last window exits, which is right for a document app.

At runtime, three things are adjustable:

```ts
await window.voussoir.tray.tooltip("3 unread");
await window.voussoir.tray.checked("sync", true);
```

**Why not retitle items?** On Android the tray is a posted notification and
cannot be edited in place. The runtime API refuses there with a reason rather
than pretending it worked, so a page that branches on `platform` stays honest.

## Devtools in release

Build with the `devtools` feature and call `.devtools(true)` on the `App`; `F12`
and `Cmd-Alt-I` then open devtools even in a release build.
That is unusual, and deliberate: an app whose UI is a webview is hard to debug
from a crash report alone, and the alternative in the wild is apps that ship with
devtools on by accident.

If you ship without it, the `devtools` feature is off and the binding does
nothing.

## Checking what a platform actually supports

```ts
if (window.voussoir.platform !== "ios") {
  // offer the menu item
}
```

Branch on `platform` for the affordance, never on an error. A feature that is
absent should not be offered, and a call that errors because a platform lacks it
is a bug in the page, not a runtime condition to handle.

## Next

- [Platform support](platforms.md) — the matrix of what is tested where.
- [Packaging](packaging.md) — the tray and menu survive packaging unchanged.
