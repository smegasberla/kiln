# Changelog

All notable changes to the `kiln` crate and the `kiln` CLI. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) with the 0.x rule
described under [Compatibility](../README.md#compatibility-policy) in the README.

## [Unreleased]

### Added

- `App::report_sink`: install an application sink that receives every crash and
  error record, so an app can forward reports to its own collector without
  forking the shell. The framework still uploads nothing by itself.
- Crash-log rotation: the log file is capped at 512 KiB and rotated to
  `<name>.old`, so a panic loop cannot fill a user's disk.
- `CHANGELOG.md` and a README "Compatibility policy" section stating what a
  breaking change is.

### Changed

- The Android release CSP's `'unsafe-inline'` is now documented on its branch as
  a first-class exception with its compensating controls and the check that must
  precede any tightening; a guard test asserts the exact Android policy on
  Android.
- The desktop↔Android parity checker warns when a command registration is not a
  statically readable string literal, and documents its known limitations.

### CI

- The Windows and macOS jobs now run `scripts/verify.ts`, so the panic
  containment, remote-script and navigation guarantees are asserted on WebView2
  and WKWebView too, not only on WebKitGTK. The harness gained a Windows `.exe`
  suffix and explicit per-platform skip reporting for Linux-only checks.

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
