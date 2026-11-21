# Changelog

All notable changes to the `kiln` crate and the `kiln` CLI. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) with the 0.x rule
described under [Compatibility](../README.md#compatibility-policy) in the README.

## [Unreleased]

### Added

- Per-window command allowlists at runtime: `WindowOptions.allowlist` (also
  accepted as `commands` from the page) narrows one window's surface, intersected
  with the global `commands` list, so a window that renders untrusted content can
  be given a strict subset without declaring its label in `kiln.json` up front.

- Multi-window bookkeeping: `AppHandle::open_window` returns the window's label,
  `__kiln_open_window` answers `{"label": "..."}`, unnamed windows get a distinct
  label (`index.html`, `index.html-2`, …) instead of colliding, `__kiln_windows`
  lists the open labels in opening order, and `kiln.windows()` wraps it. The shim
  protocol stays at 2; `windows()` is the only method protocol 2 promises.

- Runtime tray and menu state: `TrayItem::check()`/`checked(bool)` and
  `MenuItem::check()`/`checked(bool)` declare a check item, and
  `AppHandle::set_tray_tooltip`, `set_tray_checked`, `popup_tray` and
  `set_menu_checked` (plus the `__kiln_tray_*`/`__kiln_menu_checked` built-ins and
  the `kiln.tray`/`kiln.menu` shim helpers) update it after the app started. Each
  validates before queueing, so a plain item, an undeclared tray, or a platform
  whose tray is a posted notification gets an error naming the reason rather than
  a silent no-op. The native call runs on the UI thread, which is where the
  objects live.

- `kiln::state`: a shell-owned `serde_json` store that survives a hot reload, with
  `get`/`set`/`remove`/`contains`/`keys`/`clear` and `reload_generation()`. The
  hot loader bumps the generation on each successful swap, so reloaded code can
  see which library is live and keep its state instead of losing it with the
  dylib. The README's hot-reload section now shows the pattern; the 250 ms
  polling latency is documented next to it.

- `command_typed` / `App::command_typed`: a command whose argument and reply are
  concrete types. The payload is deserialized before the body runs, so a wrong
  shape is rejected as `kind: "payload"` naming the command instead of becoming a
  silently defaulted field. The raw `Value` API is unchanged and still the base
  layer; `kiln` re-exports `serde` so the derives need no extra dependency.

- The `rt` feature and `App::async_fn`: register a real `async fn` command. Its
  future runs on one shared multi-threaded runtime rather than a thread per call,
  so in-flight load costs tasks instead of stacks. A panic inside the future is
  caught and reported as `kind: "panic"` instead of leaving the page waiting
  forever. Off by default; the minimal shell pays nothing and its binary size is
  unchanged.

- The error envelope now carries `method` alongside `message` and `kind`, so a
  page can tell which command failed without parsing text. Every failure path
  (unknown, allowlist denial, bad payload, handler `Err`, panic, runtime) is
  covered by a test asserting both fields and a distinct kind.

- `on_ready` gains the window label and fires exactly once per window, including
  windows opened later; a page reload no longer fires it again.
  **Breaking:** the closure now takes `(&AppHandle, &str)`. The label is what
  makes the hook usable in a multi-window app, and adding it later would have been
  a second break; `.on_ready(|app, _window| …)` is the one-line migration.
  `on_window_close(|app, label| …)` is new and fires once as a window goes away.
  Both are documented builder methods and both are duplicate-event guarded.

- The `storage` feature (part of `full`): `storage.get|set|delete|keys|path`
  backed by a JSON file in the platform data directory, written with an atomic
  rename and capped at 4 MiB. A missing data directory or corrupt file is an error
  envelope the page can handle, never a silently empty store.

- The probe fixture and `scripts/verify.ts` now assert the typed-command rejection
  and the storage round-trip through the real IPC transport on every OS, so the
  T1 surface is proven on WebKitGTK, WebView2 and WKWebView rather than assumed.

- Per-window command allowlists: a `windows` map in `kiln.json`, keyed by window
  label, narrows what that window may call to the intersection of its own list
  and the global `commands` list. The label is resolved by the shell from the
  calling webview, never from the request, so a page cannot claim another
  window's surface; a window with no entry keeps the global allowlist.

- Install scripts: `install.sh` for Linux, macOS and the BSDs, `install.ps1`
  for Windows. Each installs the toolchain dependencies (system webview dev
  files, Git, Bun, Rust) when they are missing, builds the CLI from source —
  from the checkout when run inside one, a fresh clone otherwise — and puts
  `kiln` on the user's PATH, finishing with an execution check of the
  installed binary. Dependency steps are skipped when the tool is already
  present, and a failed package install warns instead of aborting, so
  `kiln doctor` reports any remaining gap by name.

- iOS support through the simulator, per Phases I-1 to I-4 of `IOS-PLAN.md`:
  the `kiln-ios` bootstrap crate, platform gates with honest fallbacks (menus,
  dialogs, tray and the updater's fetch decline on iOS), UIKit lifecycle mapped
  onto the shared `lifecycle` events, an `ios/` shell scaffold (staticlib,
  XcodeGen project, one-symbol bridging header), and `kiln ios build|run|verify`
  — built and round-trip-verified on a booted simulator, green in CI since run
  44788618663.
- Hot-reloadable commands behind a cargo feature: the scaffold now ships a
  `commands` crate with one `static COMMANDS: &[Command]` table (with
  `Kind::Sync` / `Kind::Async`), and `kiln dev` watches its `src/`, rebuilds
  only that crate, and swaps the new library into the running shell. The
  harness (`scripts/verify-hot.ts`) proves the swap leaves the page running —
  one `performance.timeOrigin` across command edits, a panic, a broken build
  and the recovery — with rebuild-to-reload latency printed by the shell.
  Release builds are untouched: the feature is never enabled for them, so
  `libloading` stays out of the release dependency tree.
- `App::report_sink`: install an application sink that receives every crash and
  error record, so an app can forward reports to its own collector without
  forking the shell. The framework still uploads nothing by itself.
- Crash-log rotation: the log file is capped at 512 KiB and rotated to
  `<name>.old`, so a panic loop cannot fill a user's disk.
- `CHANGELOG.md` and a README "Compatibility policy" section stating what a
  breaking change is.

### Changed

- `kiln init` scaffolds commands as a table in a separate crate instead of
  inline `.command()` calls in `main.rs`, so the same code hot-reloads in dev
  and links statically in release. `.command()` remains for commands that need
  the `App` itself; release still builds with every feature, as it did.
- Documentation now states that hot reload is desktop-only and that statics
  inside the commands crate do not survive a reload.
- The Android release CSP no longer allows `'unsafe-inline'` in `script-src`; it
  now matches the desktop release policy. The removal is backed by an emulator
  measurement (android-36, system WebView 133) in which the injected bridge shim
  round-tripped a command with the tightened policy; a guard test asserts the
  Android `script-src`.
- The desktop↔Android parity checker warns when a command registration is not a
  statically readable string literal, and documents its known limitations.
- Command parity is now three-way (desktop, Android, iOS), and a `#[cfg(test)]`
  command table no longer counts as a registration — it had made every fresh
  desktop+mobile scaffold fail parity on commands that only exist in tests.

### CI

- The Windows and macOS jobs now run `scripts/verify.ts`, so the panic
  containment, remote-script and navigation guarantees are asserted on WebView2
  and WKWebView too, not only on WebKitGTK. The harness gained a Windows `.exe`
  suffix and explicit per-platform skip reporting for Linux-only checks.

### Fixed

- WebView2 could not reach a command at all. It cannot intercept a custom
  scheme, so wry rewrites `kiln://localhost` to `http://kiln.localhost`, the
  same way it does on Android; Kiln only taught Android about the rewritten
  origin, so on Windows the shim allowlisted an origin the page never reports,
  `invoke` posted to an endpoint nothing intercepts, and the first navigation
  was refused. The page origin now follows the target, and the dev CSP, the
  probe fixture and the harness pages use it too.

- The Linux job pins `appimagetool` to the 1.9.1 release by SHA-256 instead of
  the moving `continuous` tag, and runs `cargo test --workspace` verbatim now
  that `kiln-android`'s dependencies are target-gated to Android.

## [0.1.0]

First published version.

### Added

- `App` builder with compile-time embedded config (`kiln.json`) and assets, and a
  cloneable `AppHandle` for `emit`, `emit_to`, `eval`, and `open_window`.
- One JSON IPC endpoint (`POST kiln://localhost/__kiln/<method>`), with sync
  handlers on the UI thread and `async_command` handlers on worker threads.
- Panic containment for every handler: a panicking command returns
  `{"error":"command panicked: ..."}` and the window survives.
- Navigation allowlist and Content-Security-Policy served with every response,
  plus the origin-scoped `window.kiln` shim.
- Optional features: `menu`, `tray`, `dialog`, `updater`, `devtools`, `full`.
- Crash reporting to an in-memory ring buffer, an optional log file, and the
  `__kiln_reports` command.
- `kiln-android` crate, driven by an activity that owns the event loop and uses
  wry's IPC handler instead of the custom protocol.
- CLI: `init`, `dev`, `build`, `package`, `android build`, `android run`,
  `doctor`, compiled with `bun build --compile` so end users need no JS runtime.
