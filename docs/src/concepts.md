# Concepts

This chapter is the one to read if the words "webview", "shell" or "IPC" are
not yet solid. Everything later assumes them. Each section says what the thing
is, why it exists, and what goes wrong without it. Nothing here requires you to
write code; the next chapter does that.

If you have built a web app before, you already know most of the webview half.
If you have built a Rust binary before, you know the shell half. Very few people
start with both, which is why this page exists.

## The shape of a Voussoir app

An app has two halves that run as one program:

```
  your machine, at runtime
  ┌──────────────────────────────────────────────┐
  │  one executable (native/src/main)            │
  │                                              │
  │  ┌────────────────┐     ┌─────────────────┐  │
  │  │  Rust shell    │◄───►│  OS webview     │  │
  │  │  your commands │ JSON│  your HTML/JS   │  │
  │  └────────────────┘     └─────────────────┘  │
  └──────────────────────────────────────────────┘
```

- The **shell** is a Rust program. It opens a window, decides what the page may
  load, and holds your commands, which are ordinary Rust functions.
- The **webview** is the browser engine the operating system already ships:
  WebView2 on Windows, WKWebView on macOS and iOS, WebKitGTK on Linux, the
  Android System WebView on Android. Your UI is HTML, CSS and JavaScript running
  inside it.
- The two talk over **one channel**, described below.

That is the whole architecture. There is no second process, no bundled browser,
no Node runtime in the shipped app.

## Theory: why a webview and not a bundled browser

The obvious alternative is to ship Chromium inside every app, as Electron does.
You then control the rendering engine completely, at the cost of roughly a
hundred megabytes per app, a second engine's worth of security patches, and a
second process tree to keep alive.

Using the OS webview inverts that trade:

| | Bundled browser | OS webview (Voussoir) |
| --- | --- | --- |
| Install size | Large, mostly engine | Small, mostly your code |
| Rendering | Identical everywhere | Differs by OS and version |
| Updates | You ship them | The OS ships them |
| Memory | One engine per app | Shared with the system |

Neither is wrong. Voussoir picks the webview because the shipped thing is then
a Rust binary plus your assets, and because JavaScript runs only while your page
is on screen. The cost you accept is that "identical everywhere" is gone: the
same CSS can lay out differently on WebKitGTK and WebView2. Treat the webview as
a supported target you test, not as a guarantee you design against.

**Why this matters for the rest of the book:** every command in Rust is a
function the page can call, so the page is trusted code with real authority
behind it. That is why there is a security policy chapter and why the policy is
on by default rather than opt-in.

## Theory: the single command channel

The shell and the page need to talk. The naive approach is to invent a
different call for every feature: one for file dialogs, one for the tray, one
for windows. Voussoir has exactly one call:

```
POST voussoir://localhost/__voussoir/<method>
body:     <your payload, as JSON>
response: {"ok": <your return value>}
        | {"error": {"message": "...", "kind": "...", "method": "..."}}
```

Every command, yours or built in, is reached the same way. The page sees one
function:

```js
const result = await window.voussoir.invoke("greet", { name: "you" });
```

Three properties follow, and each is the reason for a rule elsewhere in the book:

1. **A command exists only if Rust registered it.** There is no manifest, no
   capability file, no runtime discovery. If you did not write `.command(...)`,
   the page cannot call it.
2. **One envelope means one error shape.** A failed handler, a bad payload, a
   panic and a policy denial all arrive as `{error}`, distinguished by `kind`,
   so page code branches on data instead of matching strings.
3. **The registry is a chokepoint.** Because every call passes through it, one
   allowlist can gate the whole surface, per window, without the page
   cooperating.

The full contract, including every `kind`, is in
[Commands & payloads](ipc-contract.md).

## Theory: commands run on a thread you choose

A webview draws on a UI thread. If a command that takes two seconds to read a
file runs on that thread, the window stops painting for two seconds. So the
execution model is a decision you make per command:

| Model | Registered with | Runs on | Use for |
| --- | --- | --- | --- |
| Sync | `.command` | UI thread | work under a frame: formatting, lookups |
| Worker | `.async_command` | a thread pool | blocking I/O: files, sockets, subprocesses |
| Async | `.async_fn` | the shared async runtime | `await`-friendly libraries |

The default is sync because most commands are cheap and a thread hop costs
something. Picking wrong is not a correctness bug: the sync path is the one that
can make the window stutter, and the worker path is the one that can exhaust a
pool under load, which is what the `busy` error kind reports.

## Theory: your frontend is built, then it is just files

`voussoir dev` and `voussoir build` run Bun, which compiles TypeScript, JSX and
CSS into plain JavaScript and stylesheets. In a release build that output is
embedded into the executable. Nothing is installed on the user's machine and no
JavaScript engine is added: the OS webview already has one.

So a release build is not "an app next to a folder of assets". It is one file.
Deleting the folder you built from does not break the app, and there is no path
where the app finds the wrong copy of `index.html`.

## Theory: what "the app's own origin" means

The page is not loaded from `file://` and not from a website. It is served from
a custom scheme the shell answers itself:

```
production:  voussoir://localhost/...
development: http://127.0.0.1:5177/...   (the Bun dev server)
```

On Windows and Android the webview engine cannot intercept a custom scheme, so
the shell rewrites it to `http://voussoir.localhost` and intercepts that. The
page *is* still your app; it is only spelled differently. This is why pages read
`window.voussoir.pageOrigin` instead of hard-coding a base URL, and why the
navigation policy allows exactly those origins and nothing else.

The consequence to remember: **a link to `https://example.com` is not navigation,
it is a refusal.** Opening a foreign page inside your app would replace your UI
with content you do not control, in a context that can reach your commands. If
you want to show external content, open the user's browser.

## Vocabulary

| Term | Means |
| --- | --- |
| **shell** | the Rust binary: window, policy, commands |
| **webview** | the OS browser engine your page runs in |
| **shim** | the small JS layer that defines `window.voussoir`; injected, not installed |
| **command** | a Rust function the page can `invoke` by name |
| **registry** | the table of registered commands; the chokepoint |
| **envelope** | the `{ok}` / `{error}` JSON shape both directions |
| **origin** | the scheme+host the page is served from |
| **allowlist** | the set of command names a page may call |
| **hot reload** | swapping the commands library into a running dev shell |
| **entry** | the HTML file loaded first, default `index.html` |

## Where to go next

- Never built either half? [Your first app](first-app.md) walks from an empty
  directory to a window you can talk to.
- Comfortable already? Read [Commands & payloads](ipc-contract.md), then
  [Security model](security.md).
- Deciding whether this suits your project? [Platform support](platforms.md)
  and [Compatibility policy](compatibility.md) are the honest limits.
