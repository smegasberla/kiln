# Changelog

All notable changes to the `kiln` crate and the `kiln` CLI. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) with the 0.x rule
described under [Compatibility](../README.md#compatibility-policy) in the README.

## [Unreleased]

### Added

- iOS support through the simulator, per Phases I-1 to I-4 of `IOS-PLAN.md`:
  the `kiln-ios` bootstrap crate, platform gates with honest fallbacks (menus,
  dialogs, tray and the updater's fetch decline on iOS), UIKit lifecycle mapped
  onto the shared `lifecycle` events, an `ios/` shell scaffold (staticlib,
  XcodeGen project, one-symbol bridging header), and `kiln ios build|run|verify`
  — built and round-trip-verified in CI on a booted simulator.
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
