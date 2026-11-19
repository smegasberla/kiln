# Kiln — iOS Support Plan

Written 2026-11-19, after Phase C (per-window allowlists, Android CSP tightening)
landed. Everything below is grounded in what exists today; file and line
references point at the seams the work hangs on.

## Why this is now mostly a port, not a design

The Android work already built the two hard parts of mobile support:

- **The transport seam** (`kiln/src/lib.rs:893`, `pub mod transport`): a mobile
  build replaces only the *transport* (wry's IPC handler on Android,
  `WKScriptMessageHandler` on iOS) and the *entry point* (JNI vs `fn main`).
  The envelope, the registry, the allowlists (global and per-window, Phase C),
  the CSP and the navigation policy are shared code.
- **The `MessageBridge` kind** (`transport::Kind::MessageBridge`) and
  `serve_bridge(registry, handle, label, message)` already parse
  `{id, method, payload}` and answer with `__kiln_resolve(id, envelope)`. The
  iOS shim needs the same bridge shape, so the page-side shim is already
  written.

What iOS needs is the platform glue: a `kiln-ios` crate, a CLI command, an
Xcode project template, CI, and the same verify story Android has.

## Open decision to settle first (spike, one day)

**Does the protocol transport work on iOS?** On iOS wry can register custom
protocols through `WKURLSchemeHandler`. If `kiln://localhost` serving works
there, iOS can use the *same* transport as desktop (`transport::Kind::Protocol`)
and skip the message bridge entirely — pages would need no per-platform
distinction at all, and the Phase C per-window label threading (already done for
the protocol path) is inherited for free.

- Spike: build the scaffold with a tiny `main`-less harness for
  `aarch64-apple-ios-sim`, load a page that calls `greet`, log the result
  through `WKWebView` console forwarding to logcat equivalent (`NSLog`).
- If the scheme handler works: transport = Protocol, bridge stays Android-only.
- If it does not (or navigation/origin checks fight it): transport =
  MessageBridge via `WKScriptMessageHandler`, mirroring
  `cli/src/commands/android.ts`'s `WRY_ANDROID_*` expectations — wry generates
  Kotlin glue on Android; on iOS the equivalent is the `WKWebView`
  `userContentController` wiring the template's Swift files own.

The plan below assumes **Protocol-first, MessageBridge fallback**, since the
code already supports both kinds. `security.rs`'s Android branch comment
documents why Android cannot use the protocol; iOS's answer should be written
there the same way, whatever the spike says.

## Phase I-1 — the `kiln-ios` crate (the entry point)

Mirror `kiln-android/` (140 lines): a crate whose root is
`#![cfg(target_os = "ios")]`, compiling to nothing elsewhere, with
**target-gated dependencies** — the lesson from B1
(`kiln-android/Cargo.toml`'s comment; `cargo test --workspace` must stay green
on every host). Workspace membership rules from B1 apply verbatim.

Contents:

- A `pub fn run(app: App)` called from a small Swift/Objective-C entry (the iOS
  counterpart of JNI `MainActivity.onCreate`), which:
  - resolves the registry/app-handle singletons the way
    `kiln-android/src/lib.rs:38` does (`runtime()`),
  - drives the shared `run` path — window creation happens on the main thread;
    on iOS the event loop is the UIApplication run loop (tao supports iOS; the
    spike confirms the tao 0.37 window/window-handle path).
- `#[no_mangle]` extern "C" shims the template's Swift calls into.
- No `ndk`-style dependency needed; log via `os_log`/`NSLog` with the `Kiln`
  tag, mirroring `kiln_android::log`.

*Acceptance:* `cargo check -p kiln-ios --target aarch64-apple-ios-sim` green on
a macOS runner; `cargo test --workspace` stays green on Linux (the crate is
empty off-iOS, like `kiln-android` off-Android).

*Landed 2026-11-19.* The crate is in place (`kiln-ios/`, `staticlib` + `rlib`,
target-gated deps) and the CI `ios-check` job runs the device-target check on
macOS. On this Linux host, every dependency type-checks for
`aarch64-apple-ios` once `rustc` is pinned to the rustup toolchain; the only
failure left is `objc2-exception-helper`'s build script, which needs the Apple
SDK's `xcrun` — precisely the split the CI job resolves.

## Phase I-2 — platform correctness inside `kiln`

Small, already half-done pieces:

- `platform.rs:163` already has an iOS `HOME`/data-dir branch — verify it
  against the real sandbox paths (`Documents` vs app container root) and extend
  it to where the updater, crash log (`reporting.rs`), and dialogs expect to
  write.
- Feature gating by platform reality, following the scaffold's honesty rule
  (the scaffold already hides the second-window button on mobile):
  - `menu.rs`, `tray.rs`, `dialogs.rs`: menus and tray do not exist on iOS;
    dialogs become `UIAlertController` (Swift shim) or are cfg'd out first.
  - `updater.rs`: keep, but its on-disk state must live in the sandbox
    directory; document that iOS app updates come from the App Store, so the
    updater is diagnostic-only on iOS initially.
  - Second windows: iOS is single-window in practice; `__kiln_open_window` and
    Phase C per-window lists stay supported at the API level (Phase C tests are
    host-platform), but the scaffold page hides the affordance like it does on
    Android.
- Lifecycle: the `lifecycle` event already fires on Android pause/resume; wire
  the iOS equivalents (`applicationDidEnterBackground` /
  `willEnterForeground`) through the same event so the page contract is
  identical.

*Acceptance:* the scaffold's page runs on the iOS simulator with every command
it can reach answering `ok`, and each cfg'd-out feature is documented as
desktop/Android-only in README (the "supported" table at `README.md:288`).

*Landed 2026-11-19, code side.* The opt-in crates (muda, tray-icon, rfd,
ureq) are gated off iOS with the desktop deps, so a feature-`full` iOS build
no longer compiles code that cannot exist there; menus build as an inert
handle, dialogs and the tray report honest errors, and the updater's fetch
declines while the comparison logic stays shared. The data-dir branch was
already correct and is now documented as the sandbox's
`Library/Application Support`. Lifecycle needed no new platform layer: tao
maps the UIKit callbacks onto `Suspended`/`Resumed`, and the run loop turns
those into the same `lifecycle` events Android emits. Still open, and owned
by I-3/I-4: executing any of it — the simulator run that makes the
acceptance line more than code review.

## Phase I-3 — `kiln ios build|run|verify` in the CLI

Mirror `cli/src/commands/android.ts` (276 lines) command-for-command:

- `build`: bundle the frontend (`cli/src/pipeline.ts` `bundle` +
  `writeAssetTable`, same as Android), cross-compile
  `cargo build --target aarch64-apple-ios-sim` (simulator) or
  `aarch64-apple-ios` (device, signing out of scope), stage the staticlib into
  the Xcode project, run `xcodebuild` against the template project.
- Project template: an `ios/` directory generated at `kiln init`, committed
  like `android/`'s Gradle files (no build-time generation — same rule). Keep
  it XcodeGen-friendly (`project.yml`) so the CLI does not parse Xcode state.
  Contents: Info.plist (origin allowlist for the app transport security, the
  `kiln` scheme), the Swift entry file, bridging header, and the
  `WKScriptMessageHandler` glue if the spike picks the bridge.
- `parity`: extend `cli/src/parity.ts` to check desktop ↔ Android ↔ iOS
  command sets — three-way, not pairwise.
- `run`: boot the chosen simulator (`simctl`), install, launch, mirror the
  Android run's `RustStdoutStderr` logcat trick with simulator stdout.
- `verify`: the iOS counterpart of the emulator job — install the scaffold,
  assert the page's console reports the round-trip (the same
  `KILN_INVOKE_DONE`-style marker pattern used for the C2 measurement), plus
  the hostile checks where they exist on mobile (remote script blocked,
  navigation locked down — the policy is shared code, so this mostly works by
  construction, but assert it, as finding 1's discipline demands).

*Acceptance:* `bun cli/src/index.ts ios build` produces a simulator app from a
fresh `kiln init`; `ios verify` green in CI.

*Landed 2026-11-19, code side.* `kiln init` writes the `ios/` shell (staticlib
crate, `main_rs` entry, XcodeGen `project.yml`, Info.plist, `main.swift`,
bridging header) and `kiln ios build|run|verify` mirror the android commands:
build cross-compiles, stages `lib<name>.a` and runs `xcodebuild` with signing
disabled; run drives `simctl` including booting a simulator; verify asserts the
app reaches its Rust entry through `--console-pty`. Command parity is now
three-way (desktop, android, iOS), and the checker gained the fix it needed for
that to be meaningful: a `#[cfg(test)]` command table no longer counts as a
registration, which had made every fresh desktop+mobile scaffold fail parity.
On this Linux host everything up to the cargo cross-compile is exercised (the
cargo step needs the Apple toolchain); the first real execution of all three
commands is Phase I-4's macOS CI job.

## Phase I-4 — CI

Add a `ios` job mirroring the `android` job (`ci.yml:150-220`): macOS runner,
`aarch64-apple-ios-sim` target, Xcode/simctl from the runner image, `ios
build` + `ios verify`, plus the B1 `cargo test --workspace` host check already
covered by the Linux job (macOS adds the cross-check of `kiln-ios` being
empty off-iOS). No code signing, no device build in CI; both are documented
out of scope.

*Acceptance:* green iOS job on every PR; the iOS verify check fails loudly if
the transport or the shim regresses, the way the Android emulator job does.

## Phase I-5 — docs and the guard tests

- README: compatibility table row (`iOS: supported (simulator)`, with the same
  desktop-only footnotes), a Mobile section for iOS next to the Android one.
- `security.rs`: an iOS branch in `content_security_policy` only if the spike
  says the policy must differ; otherwise assert equality with desktop in a
  guard test named after what it pins (the naming lesson from the C2 rename).
- PRODUCTION-READINESS: a new dated section recording what was measured on the
  simulator and what was not (device builds, App Store flow) — the honest
  caveat style of the C2 entry.
- CHANGELOG entry under the current section.

## Order, estimates, risks

**Spike → I-1 → I-2 → I-3 → I-4 → I-5.** The spike gates the transport choice;
I-2's feature gating can proceed in parallel with I-3.

Risks, in order of how much they can hurt:

1. **tao/wry iOS support in the pinned versions.** tao 0.37 and wry 0.57 both
   advertise iOS, but the pinned versions' iOS paths are less-trafficked than
   their Android ones. If the spike hits a wall, the fallback is a wry upgrade
   (breaking-change review against `README.md:382`'s list first).
2. **The event loop must own the main thread on iOS.** `kiln-ios::run` cannot
   spawn its own loop the way desktop `fn main` does; it must return control to
   `UIApplicationMain`. This shapes the entry point and is why the spike comes
   first.
3. **Simulator-only coverage.** Device behaviour (keyboard, safe areas,
   backgrounding) is not CI-verifiable; document it like the C2 caveat.
4. **Per-window semantics on iOS.** Phase C's label plumbing is already
   transport-shared, but iOS single-window reality means the narrow-list path
   is exercised by unit tests, not device e2e — say so in the docs.

Estimates: spike 1 day; I-1 1–2 days; I-2 1–2 days; I-3 2–3 days; I-4 0.5 day;
I-5 0.5 day. Roughly a week of focused work, dominated by the toolchain
friction the Android phase already mapped.
