# Kiln — Implementation Plan

A Tauri-alternative desktop app framework. Shipped runtime = native Rust binary
(wry + tao, OS webview). Bun is build/dev tooling only and never enters the
artifact.

Priorities, in order, and the tie-breakers for every decision below:

1. **Performance** — cold start within ~10% of a bare wry+tao app; IPC overhead
   small enough to ignore.
2. **Not bloated** — every crate and package must earn its place.
3. **Extremely lightweight** — binary size close to bare wry+tao, no Chromium,
   no JS runtime, no Node.

## Repository shape

```
Kiln/
  PLAN.md                      this file
  README.md                    architecture + Tauri comparison
  Cargo.toml                   workspace (members: kiln, examples/counter)
  kiln/                        runtime shell crate (ships to end users)
    src/lib.rs                 App builder, run loop, protocol handler, emit
    src/config.rs              single JSON config, code-first defaults
    src/error.rs               one small error enum
    src/ipc.rs                 command registry + dispatch + envelope
    src/assets.rs              compile-time asset table + MIME table
    src/shim.js                the page-side `window.kiln` (inlined at compile time)
    examples/hello.rs          framework example: round-trip + emit, with a
    examples/hello/            headless `KILN_SELFTEST` mode for CI
  baseline/                    measurement baseline + transport probes
    src/bare.rs                raw wry+tao hello world (the 100% mark)
    src/probe.rs               proves protocol-body IPC and dev-mode CORS
  scripts/
    verify.ts                  9 end-to-end checks (the acceptance gates)
    bench.ts                   cold start vs the baseline
  cli/                         Bun CLI (dev/build tooling, never shipped)
    src/index.ts               arg parsing + dispatch
    src/commands/init.ts       scaffold
    src/commands/dev.ts        Bun.serve + HMR + launch shell
    src/commands/build.ts      bun build -> cargo build --release
    template/                  scaffold files, inlined into the compiled CLI
  examples/counter/            a real scaffolded app, committed as living proof
```

## Phase 0 — Probe and baseline (before any framework code)

Purpose: retire the two unknowns that could invalidate the architecture, and
capture the performance baseline every later phase is measured against.

- Probe A: does `wry` + `tao` actually run on this machine (Wayland/X11,
  WebKitGTK 2.54)? Verify a window opens and JS executes.
- Probe B: **decide the IPC transport empirically.** The brief specifies wry's
  `with_ipc_handler` (`window.ipc.postMessage`). That channel is fire-and-forget
  on every platform; returning a value requires a second hop through
  `evaluate_script`, i.e. re-parsing the response as JavaScript source. The
  alternative is a custom protocol endpoint (`POST kiln://localhost/__kiln/<method>`)
  that returns the response directly. Probe both on WebKitGTK and pick the one
  that survives; record the numbers.
- Baseline: build `examples/bare.rs` (raw wry+tao, hardcoded HTML) and record
  cold start, binary size, and dependency count. This is the bar.

**Done when:** the chosen transport is proven with a real request/response, and
the baseline numbers are written down.

**Acceptance checks owned by this phase:** the measurement methodology for
"cold start within ~10%" and "size close to bare wry+tao" exists and is runnable.

## Phase 1 — Runtime shell crate (`kiln`)

The framework itself. One public builder, one dispatch table, one protocol.

- `App::new(name)` with code-first defaults; `App::from_json` merges a small
  config string (single file: `kiln.json`) over those defaults. Config is
  embedded with `include_str!`, never read from disk at runtime.
- `App::command(name, handler)` registers a handler. Handlers take
  `(&AppHandle, Value)` and return `Result<Value, Error>`. The whitelist is
  whatever is compiled into the binary — no ACL/capability files, no plugin
  manifest, no separate permissions directory.
- `App::assets(closure)` supplies the compiled-in frontend bytes; the shell
  serves them over the custom protocol with a ~15-line MIME table (no
  `mime_guess`).
- `App::run()` owns the tao event loop. `AppHandle::emit(event, payload)` hops
  through a tao `EventLoopProxy` and runs `window.__kiln_emit` on the main
  thread — no `Rc<RefCell<Option<WebView>>>` games, no unsafe.
- Dev vs build switch: `KILN_DEV_URL` env var, honored only in
  `debug_assertions` builds. Release binaries cannot be redirected at runtime.
- Dependencies: `wry`, `tao`, `serde_json`. Nothing else. No proc-macro crate,
  no `serde` derive, no `serde` (only `serde_json::Value`).

**Done when:** crate compiles with no warnings, unit tests cover dispatch
(unknown method, happy path, handler error) and asset path resolution (traversal
rejected), and `examples/hello.rs` launches with the API.

**Acceptance checks owned by this phase:** dependency count frozen and reported;
no Node/Bun/Chromium artifact in the crate's dependency tree.

## Phase 2 — Hardcoded example app, end-to-end round-trip

`examples/hello.rs`: a real app on the public API with an embedded page that
calls `invoke("greet", {name})`, gets JSON back, and writes it into the DOM.
Verified by running it against a live webview and asserting the Rust side
observed the round-trip (JS calls a second command carrying the result, which
Rust prints) — not by eyeballing a screenshot.

**Done when:** the round-trip is proven programmatically, and a Rust->JS `emit`
reaches the page.

**Acceptance checks owned by this phase:** "verify it launches and round-trips
data" from the brief.

## Phase 3 — Measure, then decide

Re-run the Phase 0 harness against the Kiln example.

- Cold start vs bare baseline (target: within ~10%).
- Release binary size after `strip` (target: same class as Tauri's ~57MB minimal
  build, nowhere near Electron).
- `cargo tree` counts and clean-build wall time for shell and CLI.
- `strings`/`grep` sweep of the shipped binary for node/bun/v8/electron markers.

This gate exists to catch regressions while the surface is still small. If a
number misses the target, the fix lands before the CLI exists, because after
that point the same fix costs more.

**Done when:** all four numbers are recorded in README.md with the exact commands
used to produce them.

**Acceptance checks owned by this phase:** all four success criteria from the
brief.

## Phase 4 — Bun CLI (`kiln`)

Tooling only. TypeScript on Bun, compiled with `bun build --compile` so
developers install a binary with no Bun/Node at runtime.

- `kiln init [dir]` — write the scaffold: `kiln.json`, `Cargo.toml`,
  `src/main.rs`, `index.html`, `src/main.ts`, `src/style.css`. Templates are
  inlined into the CLI binary at compile time (`with { type: "text" }`), so the
  installed CLI has no asset directory to lose.
- `kiln dev` — `Bun.serve({ development: true, routes })` on the entry HTML for
  HMR, then spawn `cargo run` with `KILN_DEV_URL` pointing at it. Forwards
  signals, kills both on exit.
- `kiln build` — `bun build <entry html> --outdir dist --minify`, then
  `cargo build --release` in the app dir.
- JSON output for machine use; human output otherwise.

**Done when:** `kiln init` in a temp dir produces a project that `kiln build`
compiles, and `kiln dev` launches a window pointed at the dev server.

**Acceptance checks owned by this phase:** "compiled CLI with zero Node/Bun
runtime dependency" verified by running the compiled binary on a `PATH` with no
node/bun, and by checking its own size.

## Phase 5 — Scaffold template + committed example + README

- The template's example command round-trip (JS -> Rust -> JS -> DOM) is the
  same proof as Phase 2, but shipped as what users get from `kiln init`.
- `examples/counter/` is a project produced by the CLI and kept in-tree, so the
  template is continuously buildable rather than aspirational.
- README: architecture, the IPC contract, why no ACL files, the dependency
  budget, the measured numbers, and an explicit Tauri comparison (what Kiln
  gives up: plugins, updater, tray, multi-window, codegen, capability system).

## Phase 6 - Production hardening (implemented after the review)

The first cut of the shell proved the architecture but not that it was safe to
ship. A review found two defects that would lose user work, plus the integrations
a "real app" is expected to have. All of the following are now implemented and
covered by tests, and the first three are asserted at runtime by
`scripts/verify.ts` against a real window.

**Correctness fixes (were blockers).**

- **Panic isolation.** `panic = "abort"` is gone from every profile, and every
  handler is wrapped in `catch_unwind`. A handler that panics returns
  `{"error":"command panicked: ..."}` and the app keeps running with the user's
  unsaved state. Done when: a fixture command that indexes an empty Vec leaves
  the app serving other commands (verified through the real IPC path).
- **Navigation lockdown.** `with_navigation_handler` refuses anything but the app
  origin, the dev-server origin in debug builds, and `about:blank`. Done when:
  `location.href = "http://hostile/..."` leaves the page where it was, and the
  hostile server receives zero requests.
- **Origin-scoped bridge.** The shim checks its origin, tears itself down on a
  foreign document, and refuses `invoke` there. Rust injects the allowlist as
  JSON rather than letting JS guess.
- **CSP and headers.** Every response carries `script-src 'self'`,
  `object-src 'none'`, `base-uri 'none'`, `frame-ancestors 'none'`, plus
  `nosniff` and `no-referrer`. Dev mode relaxes only inline/eval/ws/loopback.
  Done when: a remote `<script>` does not execute, a remote `fetch` fails, and
  `eval` throws in a release build.

**Capabilities.**

- Multi-window, addressed by label; creation deferred to the event loop because
  tao handles are not `Send`.
- `async_command` running handlers on a worker thread, plus an async protocol
  responder so a slow reply never blocks painting.
- Native menu bar (`muda`), tray icon (`tray-icon`), native dialogs (`rfd`),
  update checks (`ureq` + rustls), devtools in release builds.
- Crash reporting: a panic hook writing to an optional log file and an in-memory
  ring buffer, readable via `__kiln_reports`. Nothing is uploaded.
- All of the above are cargo features, so the minimal shell stays three
  dependencies and 785 KB.

**Acceptance checks owned by this phase:** 13/13 in `scripts/verify.ts`, 60 unit
tests, cold start still within +10% of the baseline, and the scaffolded
`full`-feature app builds at 2.4 MB.

## Deferred and cut

Cut now, may return as opt-in crates:

- Notifications, global shortcuts, deep links, single-instance, auto-install
  updates. Each needs platform code or code signing that is an app decision.
- Plugin system. A plugin is a crate that registers commands; the core needs
  nothing for that.
- Capability/ACL files. The compile-time whitelist replaces them.
- Streaming IPC. Async handlers cover the case; a stream is an `emit` loop.
- `#[command]` proc-macro sugar — see the deviation note.
- Mobile targets, custom titlebars, transparency, incognito.

## Deviations from the brief, and why

1. **IPC transport.** The brief names wry's IPC handler. That handler cannot
   return a value; a `-> json` bridge on top of it needs `evaluate_script` and
   JS-source escaping for every response. Phase 0 measures both; the
   request/response transport wins unless WebKitGTK refuses it. Same single
   generic `invoke(method, payload) -> json` surface either way.
2. **No `#[command]` attribute macro.** The brief asks for a "compile-time
   whitelist, zero JSON config" and separately asks to avoid codegen requiring
   full recompiles. Runtime `app.command("name", fn)` satisfies both: only
   compiled-in handlers are reachable, nothing is configured in JSON, and adding
   a command touches one Rust line instead of a new attribute plus a
   proc-macro crate with `syn`/`quote`/`proc-macro2` in the dependency tree.
3. **Config format: JSON, not TOML.** `serde_json` is already required for IPC;
   TOML would add a parser and its dependency tree to parse maybe eight keys.
4. **Config is embedded, not read at runtime.** A shipped desktop app should not
   depend on a writable CWD to know its own window title.
5. **The dev server runs inside the CLI process.** `kiln` is distributed as a
   compiled binary, so there is no `dev-server.ts` on disk to spawn; and
   in-process means one signal stops both halves. HMR still comes from Bun's
   `development: true` HTML import.

## Phase 7 - BSD family

Targets: FreeBSD, NetBSD (OpenBSD, DragonFly share the same code path but have no
tier-1 Rust target or CI runner, so they are labelled unsupported until someone
runs them).

Why this is cheap: wry and tao *already* treat BSD as a webkitgtk/GTK platform.
wry's build script emits its `gtk` cfg for `freebsd|netbsd|openbsd|dragonfly`,
the same ones tao routes to its GTK backend, so the webview and window layers
need no new code. Kiln's own build script already classifies BSD as GTK-based.

What actually blocked it (both found by cross-checking, both fixed):

- **`dbus` was turned on by tao's default features.** It is used only for Linux
  portal/drag integrations that Kiln does not touch, and it needs libdbus headers
  on the build host. Disabled with `default-features = false`, keeping
  `rwh_06` + `x11`.
- **`gtk` was declared for Linux only.** The menu is a GTK container on every
  webkitgtk platform, so the target table now lists the four BSDs.

What is left, and cannot be done from Linux: the `-sys` crates (`glib-sys`,
`gtk-sys`, `webkit2gtk-sys`) run `pkg-config` against the *host*, so the build
only links where GTK 3 and WebKit2GTK are installed. That means a real FreeBSD
box or a FreeBSD CI runner, not a cross-compile.

**Done when:** CI (or a VM) builds and runs `scripts/verify.ts` on FreeBSD, and
`kiln package` produces something installable there. Installers on BSD are ports
and pkg, not AppImage; that is a separate piece of work from the runtime.

**Cut:** OpenBSD and DragonFly release support (same code, no CI, no pkg target
in this repo). Soft-rendering WebKit is out of scope on all three.

## Phase 8 - Mobile (Android first, then iOS)

This is not "another cargo feature". A mobile app is a different artifact and a
different lifecycle, and the honest estimate is weeks, not hours.

### What has to change structurally

| Concern | Desktop | Android |
| --- | --- | --- |
| Entry point | `fn main` | `android_main` via JNI, driven by `tao-macros` |
| Window | Kiln creates one | the activity owns it |
| Assets | `include_bytes!` served over `kiln://` | APK `assets/`, served over `https://` via WebViewAssetLoader |
| IPC | custom protocol POST | `@JavascriptInterface` bridge (custom schemes are awkward in Android WebView) |
| Backgrounding | app either runs or exits | paused/resumed/destroyed, state can be lost at any moment |
| Keyboard | free | on-screen IME, resize/viewport handling |
| Packaging | AppImage/.app/.zip | AAB/APK, Gradle, signing key, SDK/NDK versions |

iOS is a further step on top: no JNI, an Xcode project, `WKWebView` inside a
`UIViewController`, ATS rules, and the simulator/device split.

### Plan

1. **Split the shell from the bootstrap.** `App` currently owns the event loop.
   Factor the loop, registry and protocol handling into a `Shell` that both a
   desktop `run()` and a mobile entry point can drive. No behaviour change; this
   is the prerequisite for everything else.
2. **Android bootstrap crate** (`kiln-android`): JNI entry, activity glue,
   asset loader, `@JavascriptInterface` bridge replacing the protocol endpoint.
   Same `invoke(method, payload) -> json` surface, so app code does not change.
3. **Lifecycle.** Expose `on_pause`/`on_resume`/`on_destroy` as events
   (`kiln.listen("lifecycle", ...)`), and make it explicit in docs that a
   command may be interrupted by process death. Async handlers need a
   cancellation story before this is trustworthy.
4. **`kiln android` CLI commands.** `dev` (adb + Gradle + logcat), `build`
   (AAB), and the SDK/NDK discovery that `doctor` already reports on.
5. **iOS** only after Android is solid: Xcode project generation, same shell,
   same bridge through `WKScriptMessageHandler`.

### Deliberate limits for v1 of mobile

- One window. Multi-window, tray, menu bar, updater and native dialogs are
  desktop-only; the app must compile with them compiled out.
- No background execution, no notifications beyond the web API.
- Android target ABI set starts at arm64-v8a only; adding ABIs is a Gradle line.

**Done when:** `kiln init --mobile` produces a project that installs and runs on
an emulator, round-trips a command, survives a rotate, and `kiln build` emits a
signed-debuggable AAB.

**Not promised:** a working iOS build before Android is done. Claiming iOS
support without a Mac and a device is exactly the mistake this project already
made once with Windows and macOS.

### Phase 8 status: where this actually stopped

Android is **structurally working and empirically partial**. Recorded honestly so
the next person does not repeat the work or over-trust it.

**Working, verified on an emulator (Android 14, Chromium WebView 113):**

- `kiln` compiles for `aarch64-linux-android` (9 warnings, no errors).
- `kiln-android` compiles: JNI entry points, activity-create hook, `invoke` bridge.
- A real APK builds (7.0 MB debug, includes `lib/arm64-v8a/libkiln_example.so` and
  `assets/index.html`).
- The APK installs and launches. The activity reaches `onCreate`, `android_main`
  runs, `WryActivity` creates a WebView, and Chromium starts its processes.
- No crash, no `UnsatisfiedLinkError`, activity stays resumed.

**Not yet working:**

- The page does not finish loading its asset. `with_asset_loader` registers the
  interceptor, but the document never arrives, so no `invoke` reaches Rust (the
  Rust-side `KILN_ROUNDTRIP` log never fires). The last thing confirmed is
  Chromium starting; the first thing unconfirmed is whether the asset URL
  (`http://kiln.localhost/index.html`) is intercepted at all.

**What was learned the hard way, worth keeping:**

1. tao's `android_binding!` expands to a block, so it must be invoked *inside* a
   function, not at module scope. `#[no_mangle]` items still export from there.
2. tao and wry each generate a JNI class, and both must be spelled the same way:
   `$domain` = all but the last package segment, `$package` = the last. The
   `$activity` name must be `Rust`, because that is the class wry's Kotlin
   declares.
3. The Kotlin plugin, `androidx.webkit`, `androidx.appcompat`,
   `androidx.activity`, `androidx.lifecycle`, `android.useAndroidX=true` and
   `buildConfig true` are all required before wry's generated Kotlin compiles.
   None of this is optional and none of it is documented in one place.
4. `MainActivity` must extend wry's `WryActivity`, and the Rust library must be
   loaded before `super.onCreate` reaches it.

**Next concrete step:** verify the asset loader intercepts, e.g. by pointing the
webview at `data:` with a trivial inline page to separate "webview works" from
"asset serving works". That is a bounded debugging session, not a redesign.

**iOS:** not started. It shares the transport seam but needs an Xcode project and
a Mac.
