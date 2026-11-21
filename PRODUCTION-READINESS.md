# Kiln — Production-Readiness Findings

Review of the full source tree: `kiln/` (shell runtime), `kiln-android/`, `cli/`, `probe/`,
`baseline/`, `scripts/verify.ts`, and both GitHub workflows.

**Verified on this machine at review time**

| Check | Command | Result |
| --- | --- | --- |
| Rust unit tests (all features) | `cargo test -p kiln --features full --lib` | 71 passed, 0 failed |
| CLI typecheck | `cd cli && bun run typecheck` | clean |
| CLI unit tests | `cd cli && bun test` | 44 passed, 0 failed |
| e2e guarantee harness | `bun scripts/verify.ts` | **not run** — needs a display; CI runs it under Xvfb on Linux only |
| macOS / Windows / Android | — | **not run** — the reviewer machine is Linux |

**Overall:** no blockers (nothing exploitable, data-losing, or guaranteed to fail). All seven
findings below have since been fixed and are kept as the record of what was found: 93caa52
closed findings 1, 2, 4, 5, 6 and 7, f0d893e added finding 3's per-window allowlist, and
8c94631 completed finding 1 after an emulator measurement tightened the Android CSP.

---

## 1. HIGH — Android release CSP allows inline scripts

**Where:** `kiln/src/security.rs:54-55`

The desktop release CSP is `script-src 'self'` with `frame-ancestors 'none'`. The Android
policy as shipped is `script-src 'self' 'unsafe-inline'`.

The code comment frames the `unsafe-inline` relaxation as "scoped to the dev origin, never to
a shipped app" — but on Android that *is* the shipped policy, so the comment and the behaviour
disagree.

**Impact.** Any XSS in an Android app's own page can execute inline in a context that holds the
IPC bridge, i.e. it can call every command in the allowlist. The origin lockdown (only
`http://kiln.localhost`) limits how the payload gets in, and the Android webview may not honour
`frame-ancestors` the same way desktop engines do — but this is a genuine weakening of the
module's own stated principle.

**Fix direction.** Either
- find the wry / Android WebView mechanism that makes `script-src 'self'` work there (the
  reason `'unsafe-inline'` was needed is not documented anywhere in the module), or
- keep the relaxation but promote it to a first-class, documented Android exception with the
  reason, the residual risk, and a threat-model note in the comment block.

**Acceptance:** the comment and the shipped policy agree; a test asserts the exact Android
release CSP string.

---

## 2. MEDIUM-HIGH — the headline security guarantees are e2e-proven on Linux only

**Where:** `scripts/verify.ts`, `.github/workflows/ci.yml:73-74` (Linux/Xvfb), `:76-95`
(Windows), macOS/FreeBSD jobs

The e2e harness that proves panic containment, remote-script blocking, navigation lockdown, and
"no JS runtime in the binary" runs under Xvfb on the Linux job, against WebKitGTK. Windows
builds and runs unit tests only; macOS and FreeBSD likewise.

The pure policy functions (`navigation_allowed`, `csp()`) are platform-independent and
unit-tested everywhere, so the *logic* is covered. What is asserted nowhere is whether WebView2
and WKWebView actually **honour** the custom-protocol response headers, the CSP, and the
navigation veto the way WebKitGTK does — and custom-scheme handling is precisely where these
engines diverge. [INFERENCE: this follows from the engines' documented differences, not from a
measured failure — no macOS/Windows machine was available for this review.]

**Fix direction.** Add one `bun scripts/verify.ts` run to the Windows and macOS CI jobs (the
harness already gates on exit code and already tolerates headless environments on Linux). If a
check is genuinely Linux-specific, make the harness report it as skipped-with-reason per
platform rather than absent.

**Acceptance:** each native CI job either runs the e2e harness green or prints an explicit
per-check skip line naming the platform limitation.

---

## 3. MEDIUM — the allowlist can only narrow, never restrict by caller

**Where:** `kiln/src/config.rs:20-22`, `kiln/src/ipc.rs`

`kiln.json`'s `commands` array narrows the compiled-in handler set, and built-ins
(`__kiln_open_window`, `__kiln_close_window`, `__kiln_devtools`, `__kiln_reports`, …) are
subject to the same flat list. Once the page can call a command, **any** script running in that
page can call it. There is no capability or ACL layer of the kind Tauri provides (per-window or
per-origin scopes).

This is defensible for the declared scope: the CSP makes remote script the only vector, and
there is no untrusted-content story in the docs. It becomes the wall Kiln hits first for apps
that render untrusted content — a markdown preview, a viewer of user-supplied HTML, an embedded
OAuth page — because those inherit the full command surface.

**Fix direction.** Cheapest useful step: allow the allowlist to be per-window (config keyed by
window label) so a window showing untrusted content can be given a reduced surface without
forking the app. A full ACL is a larger design; the per-window narrowing is the version that
keeps the "minimal shell" promise.

**Acceptance:** a documented, tested path exists to run a second window with a strict subset of
the commands the main window has.

---

## 4. MEDIUM — crash reporting is in-memory + optional local file, nothing else

**Where:** `kiln/src/reporting.rs:1-10` (philosophy), `:53-57` (file write), `:111-116`
(bounded ring buffer)

"Do not phone home" is the right default for a framework, and the in-memory ring buffer is
correctly bounded. What is missing:

- no structured logging (levels, targets, timestamps usable by a log collector),
- no log rotation or size cap on the crash file (see finding 6),
- no hook for an application to install its own reporter, so every app that wants telemetry
  forks this module or reimplements it.

**Fix direction.** Add a `set_reporter(impl Fn(&CrashReport) + Send + Sync)` style hook (or a
trait object) on the builder, keep the default as the current no-op-plus-local-file, and let
apps opt into Sentry/OTLP/whatever themselves. That preserves the philosophy while removing the
fork.

**Acceptance:** an app can forward crash entries to its own sink without modifying `kiln`.

---

## 5. LOW-MEDIUM — the parity checker is regex-shaped and can be fooled

**Where:** `cli/src/parity.ts:20-31`

The desktop↔Android parity check parses Rust source with regexes after comment stripping. It
follows `include!` one level deep (and says so). Consequences:

- a command name built dynamically (`concat!`, a `const` array iterated in a loop) is invisible
  to it, so a genuine mismatch would pass;
- a string literal containing `.command("x"` can false-positive;
- it is a tripwire, not a parser, and will eventually miss something real.

It exists because a real bug shipped (`parity.test.ts:33-39` documents the `slow_task`
incident), and it already runs before `kiln android build`, so the value is proven — the risk is
a future maintainer treating it as a guarantee.

**Fix direction.** At minimum, add a `// KNOWN LIMITATION:` note naming the dynamic-name case and
the one-level `include!` depth, so the next reader knows what it cannot see. Optionally, make
the checker *fail loudly* when it encounters a `.command(` whose argument is not a bare string
literal.

**Acceptance:** the limitation is documented in-file, and a dynamic command registration
produces a warning rather than silence.

---

## 6. LOW — the crash-log file is opened per entry, append-only, forever

**Where:** `kiln/src/reporting.rs:53-57`

Every `record()` re-opens the crash file and appends, with no size cap and no rotation. The
in-memory buffer is bounded (`:111-116`); the file is not. An app that panics in a loop grows
the file without bound — a slow disk-fill on a user's machine, and the kind of thing that shows
up in a bug report months later.

**Fix direction.** Cap the file (e.g. truncate-at-start if over N MB, or keep `.log` + `.log.1`),
and/or only write the file when the in-memory buffer flushes. Cheap, local, no API change.

**Acceptance:** a test drives more entries than the cap allows and asserts the file stays bounded.

---

## 7. LOW — API stability is unversioned

**Where:** `kiln/Cargo.toml`, no `CHANGELOG.md` in the repo

The crate is `0.1.0` with no changelog and no stated semver policy. App authors pinning
`kiln = "0.1"` have no documented promise about what `0.2` may break — and in Cargo's 0.x
rules, `0.2` is allowed to break anything.

**Fix direction.** Add `CHANGELOG.md` (Keep a Changelog format is fine) and a short
compatibility paragraph in the README: what counts as a breaking change (command names,
`App`/`AppHandle` signatures, the `kiln.json` schema, the `window.kiln` shim surface) and what
does not (internal modules, feature names).

**Acceptance:** a contributor can answer "is this breaking?" from the docs alone.

---

## Non-defects worth recording

These were examined and are correct as-is; listing them so a future review does not re-open
them:

- **Panic containment** is real, not decorative: `catch_unwind(AssertUnwindSafe)` wraps handlers,
  the release profile deliberately avoids `panic = "abort"`, and the probe fixture exercises a
  panicking command end-to-end through the real IPC path.
- **Navigation allowlist** has tests for the actual prefix-match traps (`kiln://localhost.evil.com/`,
  `kiln://localhost@evil.com/`), and asset lookup cannot traverse (`assets.rs:70-73`).
- **The updater refuses to download or install** by design, and never claims an update on
  unparseable input (`updater.rs:155-159`). Correct scope for a framework.
- **CI hygiene** is above average: `RUSTFLAGS: -D warnings`, clippy `-D warnings`, a real FreeBSD
  VM rather than a cross-compile guess, Android instrumentation tests on a booted emulator, and
  the NetBSD job is *labelled* compile-only instead of passing silently.
- **Deliberate minimalism** (MIME table over `mime_guess`, hand-rolled PNG encoder for fallback
  icons) is justified in comments with the tradeoff named each time — opinionated design, not
  accreted cleverness.

---

## Suggested fix order

1. Finding 1 (Android CSP) — smallest change, highest severity, closes a claim/behaviour gap.
2. Finding 2 (e2e on Windows/macOS) — no code change needed in the shell, only CI and a
   skip-with-reason path in the harness.
3. Finding 4 + 6 together — same file, one PR, and 6 is a prerequisite for 4 being useful in
   production.
4. Finding 3 — a design decision; decide whether per-window allowlisting is in scope before
   writing code.
5. Findings 5 and 7 — documentation-level, cheap, and prevent future confusion.

---

## Fix status

Status as of the fix pass that followed this review. Findings 1–5 and 7 are
addressed; finding 3 is now fixed by the per-window allowlist (2026-11-19).

| # | Severity | Status | Where |
| --- | --- | --- | --- |
| 1 | HIGH | **fixed** — the Android exception was removed rather than documented: the release policy ships `script-src 'self'` after the emulator measurement under C2 showed the bridge survives it. The policy is a named const (`ANDROID_RELEASE_CSP`) pinned by `the_android_release_string_is_pinned_where_every_target_runs` on every target, not only where the Android branch compiles, and the contradicting module comment is gone | `kiln/src/security.rs` |
| 2 | MEDIUM-HIGH | **fixed and proven on all three engines** — Windows and macOS jobs run `bun scripts/verify.ts`, green on WebKitGTK, WKWebView (run 44788618663) and WebView2 (run 44901970031). WebView2 needed a real fix first: it rewrites the page origin like Android, which `f5b70d0` made the page origin follow. The harness gained a `win32` `.exe` suffix, a loud Linux-only skip for the `ldd` composition check, and per-platform skip reporting | `scripts/verify.ts`, `kiln/src/security.rs`, `kiln/src/lib.rs`, `.github/workflows/ci.yml` |
| 3 | MEDIUM | **fixed** — `kiln.json` gains a `windows` map keyed by window label; a listed window's surface is the intersection of its list and the global allowlist, resolved from the calling webview, never from anything the page sends. Unit tests cover narrowing and the no-widening rule; `verify.ts` opens a second window with an empty list and asserts the denied envelope | `kiln/src/config.rs`, `kiln/src/ipc.rs`, `kiln/src/lib.rs`, `scripts/verify.ts` |
| 4 | MEDIUM | **fixed** — `reporting::set_reporter` installs an application sink that receives every record, so telemetry no longer requires forking the module; philosophy (nothing phones home by default) unchanged | `kiln/src/reporting.rs` |
| 5 | LOW-MEDIUM | **fixed** — `unreadableRegistrations()` surfaces dynamic-name registrations in the parity result message, and the parser's known limitations are documented in-file | `cli/src/parity.ts` |
| 6 | LOW | **fixed** — the crash log rotates at 512 KiB to `<name>.old` (the previous file survives); tested by `the_log_file_stays_bounded`, which drives more entries than the cap and asserts the bound and the rotation | `kiln/src/reporting.rs` |
| 7 | LOW | **fixed** — `CHANGELOG.md` added (Keep a Changelog), README gained a "Compatibility policy" section naming what is and is not breaking | `CHANGELOG.md`, `README.md` |

Caveats kept honest: finding 2's fix is CI-only, and it has now proved itself on
both engines. WKWebView passed in run 44788618663; WebView2 passed in run
44901970031 after `f5b70d0`, which is the run where all eight jobs went green.
That first Windows run was not noise: it exposed a real bug — WebView2 cannot
intercept a custom scheme, so wry rewrites the page origin there the way it does
on Android, and Kiln had only taught Android about it, so no command was
reachable at all on Windows until `f5b70d0` made the page origin follow the
target. No `runtime behaviour on Windows is unverified` caveat remains, and the
guarantees (panic containment, remote script/fetch/eval blocked, navigation
locked down, second-window narrowing) are asserted on all three webview engines.
The Android CSP exception that finding 1's fix documented was removed after the
emulator measurement recorded under C2, so that caveat is closed too.

---

## Remediation plan — second review findings

A small, phased plan for the findings of the follow-up whole-project review
(hot-reload feature work is tracked separately in `PLAN.md`). Phases are
ordered so each one leaves the tree greener than it found it; A and B are
mechanical, D is a measurement, C is design work and needs its decisions
taken before code.

**Status (2026-11-19):** A1, A2, A3, B1, D1 and C1 are implemented and verified —
the workflow pins appimagetool 1.9.1 by SHA-256, the benchmark table carries
its date and framing, `cargo test --workspace` is green on Linux and runs in
CI, the table's numbers come from a fresh `bun scripts/bench.ts`, and a second
window can be given a strict command subset from `kiln.json` (tested and
documented). C2 is also landed: the Android release CSP now ships
`script-src 'self'`, verified against a scaffold probe on the android-36
emulator, and the guard test pins it.

### Phase A — supply-chain and framing (Low, all mechanical)

- **A1: pin appimagetool.** `.github/workflows/ci.yml:68-69` downloads
  `releases/download/continuous/…`, an artifact that can change under us with
  no checksum. Pin to a tagged release and `echo "<sha256>  <file>" | sha256sum -c -`
  before `chmod`.
  *Acceptance:* CI fails if the downloaded bytes drift; no `continuous` URL
  remains in the workflow.
- **A2: `Cargo.lock` — verified, no action beyond keeping it honest.** The
  second review claimed the lockfile was absent; `git ls-files` says it is
  tracked and the CI cache key already hashes it (`hashFiles('**/Cargo.lock')`
  in `ci.yml`), so the claimed gap does not exist. What remains is discipline:
  keep it committed (the hot-reload work already updates it for `libloading`),
  and re-check the cache key only if the workflow's `hashFiles` is ever
  removed. *Acceptance:* `git ls-files Cargo.lock` keeps printing the path —
  which it does.
- **A3: frame the README benchmark honestly.** `README.md:49-54` presents
  one machine's numbers as project facts. Label the table with the machine,
  the date, and the command that reproduces it (it already exists:
  `bun scripts/bench.ts`).
  *Acceptance:* a reader can tell the numbers are a sample, not a guarantee.

### Phase B — workspace correctness (Medium)

- **B1: make `cargo test --workspace` work on a clean host.** Root cause
  confirmed by reading `kiln-android/Cargo.toml`: `jni`, `ndk` and `tao` are
  unconditional dependencies, and `ndk` pulls `ndk-sys`, whose lib.rs
  `compile_error!`s on anything but Android — so the documented, natural
  contributor command fails before running a single test, while CI silently
  tests only `-p kiln`. Fix by target-gating the Android crate's deps
  (`[target.'cfg(target_os = "android")'.dependencies]`) or excluding
  `kiln-android` from default workspace members with a comment saying why.
  *Acceptance:* `cargo test --workspace` is green on a Linux host, and a CI
  step runs exactly that command so the claim stays proven.

### Phase C — design decisions (Medium, needs a decision first)

- **C1: per-window command allowlisting** (this document's open finding 3,
  re-raised by the second review). The direction stands: key the allowlist by
  window label in `kiln.json`, resolve the caller's window at dispatch (wry's
  protocol callback identifies the webview), and narrow `permitted()` — note
  that the hot-reload path added since then goes through the same
  `permitted()` gate, so it inherits the scoping for free.
  *Acceptance:* the same as finding 3 — a second window can be given a strict
  subset, tested, documented.
- **C2: Android CSP tightening — landed (2026-11-19).** The probe ran on the
  android-36 emulator (system WebView 133.0.6943.137): a scaffolded app whose
  page calls a command on load round-tripped with `'unsafe-inline'` removed
  from `script-src`, and nothing it needs was blocked. The tightening is in
  `kiln/src/security.rs` and the guard test now asserts the Android
  `script-src 'self'`. Honest caveat: one emulator image, one WebView version;
  if an older WebView regressed this, the symptom is a dead `window.kiln` on
  first load, which the scaffold page makes loud.

### Phase D — measurement refresh

- **D1: re-run `bun scripts/bench.ts`** on a quiet machine and fold the
  results into the A3 table (this is what makes A3 truthful rather than
  reworded).

**Suggested order:** A (one sitting, no risk) → B (one PR, unblocks every
future contributor) → D (minutes) → C1 (design, then code) → C2 (needs an
emulator session).

## What the iOS CI job measures — green since run 44788618663 (2026-11-20)

Phases I-1 to I-5 of `IOS-PLAN.md` are landed, and the CI macOS runner has
executed the whole chain. The job first failed (run 44758659109); two real bugs
came out of it — the simulator console was being joined as byte values so no
marker could ever match, and tao's `create_window` aborts before
`UIApplicationMain` has run — and both are fixed. What follows is now a
measurement, not a description:

- a fresh `kiln init` builds for the simulator: frontend bundled and embedded
  in the staticlib, staged, linked through the XcodeGen-generated project, and
  an unsigned `.app` produced;
- `kiln ios verify` installs and launches that app on a booted simulator and
  observes both console markers — `KILN_IOS_START` (the staticlib reached its
  Rust entry) and `KILN_IOS_ROUND_TRIP` (the page loaded from the app's own
  scheme, called a command, and was answered) — the full transport;
- command parity is checked three-way on the fresh scaffold, and a
  `#[cfg(test)]` command table no longer counts as a registration;
- the workspace test suite is green on macOS, which doubles as proof that the
  mobile-gated crates compile to nothing off mobile.

Not measured, on purpose, in the honest-caveat style of the C2 entry:

- device builds and signing, and the App Store flow — installing on hardware
  needs the app owner's provisioning profile and stays with them;
- device-only behaviour: keyboard, safe areas, backgrounding beyond the
  lifecycle event reaching the page, and performance on real hardware;
- per-window command allowlists on iOS: the platform is one window by design,
  so the narrowing path is covered by unit tests and desktop e2e only.

The simulator is the only target a CI runner can install into; everything
device-shaped is documented, not assumed.
