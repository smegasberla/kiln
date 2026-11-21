# The frontend

The page half of a Voussoir app has no framework and no required build step
beyond Bun. This chapter explains what Bun does with your files, what the shim
gives you, and which patterns work when your "browser" is a webview that cannot
navigate anywhere.

## Theory: what Bun does, and what it does not

`voussoir dev` and `voussoir build` both run one command:

```
bun build index.html --outdir dist --target browser
```

That single line is the whole frontend pipeline. Bun reads `index.html`, follows
`<script src>` and `<link rel="stylesheet">`, resolves the TypeScript/JSX/CSS
those files import, and writes a `dist/` of plain, browser-ready files. Release
builds add `--minify`.

What this means in practice:

- **TypeScript needs no config.** No `tsconfig.json` is required for bundling;
  Bun strips types and bundles. You add a `tsconfig.json` only if you want type
  checking in your editor, which is a separate concern.
- **You can import CSS from a script** (`import "./style.css"`), and it lands as
  a real stylesheet.
- **There is no `node_modules` in a release build.** Dependencies you import are
  bundled in. Node built-ins do not exist in the webview, so a library that
  requires `fs` will fail at runtime, not at build time.
- **The entry is HTML, not JS.** That is why `voussoir.json` needs an `entry`
  naming a page rather than a script.

`dist/` is then turned into `native/src/assets.rs` by the same command, so the
bundled files are what the binary embeds. You never copy `dist/` anywhere.

## The shim: what `window.voussoir` is

Before any of your scripts run, the shell injects a small JavaScript file into
every page. It defines one global:

```ts
window.voussoir = {
  invoke,      // call a Rust command
  listen,      // subscribe to an event Rust emits
  windows,     // the open window labels
  tray,        // { tooltip, checked, popup }
  menu,        // { checked }
  platform,    // "linux" | "windows" | "macos" | "android" | "ios" | ...
  pageOrigin,  // the origin this page is really served from
  protocol,    // the shim's own version, for feature detection
  timeout,     // the default command deadline in ms
};
```

Nothing imports it and nothing installs it. It is also not a DOM element and not
a module: it exists on `window` before your first line of code.

**Why a shim rather than exposing the transport directly?** There are two
transports (a custom protocol on desktop and iOS, a `postMessage` bridge on
Android). The shim is the one place that knows which, so page code calls
`invoke` and never learns which platform it is on.

### `invoke`

```ts
const value = await window.voussoir.invoke<T>("method", payload, options?);
```

- Resolves to whatever the Rust handler returned inside `Ok`.
- Rejects with `{ kind, method, message }` on failure. `kind` is the part you
  branch on; see [Errors the page can branch on](errors.md).
- `options.timeout` overrides the deadline for one call, in milliseconds. `0`
  means no deadline.

The payload is serialized with `JSON.stringify`, so it must be JSON-serializable.
A `Map`, a `Date` or a class instance will not survive; send a plain object or an
ISO string.

### `listen`

```ts
const off = window.voussoir.listen<{ dark: boolean }>("theme", (payload) => {
  document.documentElement.dataset.theme = payload.dark ? "dark" : "light";
});

// later
off();
```

`listen` returns its own unsubscribe function, which matters because the page can
outlive your component. Rust reaches this with `app.emit("theme", ...)` or
`app.emit_to(Some("main"), ...)`.

The event name is a plain string, and nothing validates it on either side. A typo
produces a listener that never fires, with no error. Keep event names in one
place in your own code.

### Platform detection

```ts
if (window.voussoir.platform === "android") {
  // no tray, no menu bar
}
```

Use this for **affordances**, not for behaviour. A command behaves the same
everywhere; a tray icon does not exist on iOS and a menu bar does not exist on
Android. Branch where the UI differs, never around `invoke`.

### Feature detection

```ts
if (window.voussoir.protocol >= 2) {
  const open = await window.voussoir.windows();
}
```

`protocol` is the shim's version, not the crate's, so it changes only when the
page-visible API does. Compare numbers, do not probe for methods.

## Theory: the page cannot navigate, and that is a feature

In a normal web app, `window.location = "https://example.com"` leaves your code
and loads someone else's. In a Voussoir app that navigation is **refused by the
shell** and recorded, because there is no back button and no address bar: the
user would be stranded in a page that is not your app.

Practical consequences:

| You want to… | Do this |
| --- | --- |
| Show a web page | `window.open(url)` / let the shell hand it to the OS browser |
| Link to docs | an `<a target="_blank">`, which the shell opens externally |
| Change views | render a different view in the same document |

A relative link inside your bundled assets still works: it is the same origin. A
link to `https://…` does not. See [Security model](security.md).

## Routing without a router

Because navigation is refused, a client-side router must not assign to
`location`. Two patterns that work:

```ts
// 1. History-less: keep the view in memory.
let view: "list" | "detail" = "list";
function render() {
  document.querySelector("#app")!.innerHTML =
    view === "list" ? listHtml() : detailHtml();
}
```

```html
<!-- 2. Real navigation inside the app: these URLs are same-origin. -->
<a href="/settings.html">Settings</a>
<!-- And in voussoir.json, per-wrapper allowlists keep "/settings.html" internal. -->
```

Pattern 2 works because the assets are embedded: `/settings.html` is served from
`voussoir://localhost`, so it is not a foreign origin. It costs a full page load,
which loses in-memory state, so it suits window-level pages rather than tabs.

## Communicating in the other direction

Rust pushes with events; the page pulls with commands. The choice is about who
knows the state changed:

- **Pull** when the page asks ("what is the current document?"). Retry and error
  handling stay in one place.
- **Push** when Rust knows first (a file changed on disk, a timer fired). Use
  `app.emit_to(...)` from `on_ready` to seed the page, then events for updates.

Never poll from `on_ready` with a timer: the page can ask, and a timer eventually
races the window closing. [Lifecycle hooks](lifecycle.md) has the ordering
guarantees.

## TypeScript types across the boundary

`invoke<T>` and `listen<T>` are your assertions, not checks. Rust does not send
types and cannot enforce them. Two habits keep this honest:

1. Define the payload shape next to the Rust struct it mirrors, and change both
   in one commit.
2. Prefer a typed Rust command (`command_typed`) so a bad shape fails on the Rust
   side with `kind: "payload"` instead of reaching your handler as a surprise.
   See [Typed commands](typed-commands.md).

`voussoir init` writes `src/voussoir.d.ts`, which declares `window.voussoir` for
the editor. It is a convenience file: if it drifts, the runtime is unaffected.

## Assets and URLs

- Reference assets relatively (`./logo.png`), not with an absolute origin.
- Inline small images as `data:` URLs where the CSP allows it; the policy permits
  `data:` and `blob:` for `img-src`.
- On Windows the origin string differs from the other desktop platforms, so an
  absolute URL built from a literal is wrong there. Use `pageOrigin` if you truly
  need one: [The two origins](origins.md).

## What is not available

| Missing | Why | Alternative |
| --- | --- | --- |
| Node built-ins (`fs`, `path`) | there is no Node in the webview | a Rust command |
| Remote script tags | the CSP forbids them | bundle the dependency |
| `eval` / `new Function` | the CSP forbids them | ship the code |
| Cross-origin `fetch` | `connect-src 'self'` | a Rust command does the fetch |
| Cookies for a remote origin | there is no remote origin | `storage.*` commands |

Every row is a deliberate trade: the page has less reach, and Rust has the
authority. If you find yourself fighting this list, the feature belongs in Rust.

## Next

- [Commands & payloads](ipc-contract.md) — the contract `invoke` speaks.
- [Storage](storage.md) — small persistent state without a database.
- [The two origins](origins.md) — why the origin string differs by platform.
