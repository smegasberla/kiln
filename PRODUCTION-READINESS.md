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

**Overall:** no blockers (nothing exploitable, data-losing, or guaranteed to fail), but seven
findings below stand between the current state and broader production use. Findings 1 and 2
are the only ones that gate shipping outside the declared scope; both are small, focused PRs.

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
addressed; finding 3 remains a design decision, deliberately not coded yet.

| # | Severity | Status | Where |
| --- | --- | --- | --- |
| 1 | HIGH | **fixed** — `unsafe-inline` promoted to a documented first-class Android exception with compensating controls and a guard test (`android_policy_documents_its_inline_exception`, which asserts the exact Android release policy on Android and no-ops elsewhere); the contradicting module comment corrected | `kiln/src/security.rs` |
| 2 | MEDIUM-HIGH | **fixed in CI** — Windows and macOS jobs now run `bun scripts/verify.ts`; the harness gained a `win32` `.exe` suffix, a loud Linux-only skip for the `ldd` composition check, and per-platform skip reporting | `scripts/verify.ts`, `.github/workflows/ci.yml` |
| 3 | MEDIUM | **open (design decision)** — per-window allowlisting needs a scope decision before code; unchanged | `kiln/src/config.rs`, `kiln/src/ipc.rs` |
| 4 | MEDIUM | **fixed** — `reporting::set_reporter` installs an application sink that receives every record, so telemetry no longer requires forking the module; philosophy (nothing phones home by default) unchanged | `kiln/src/reporting.rs` |
| 5 | LOW-MEDIUM | **fixed** — `unreadableRegistrations()` surfaces dynamic-name registrations in the parity result message, and the parser's known limitations are documented in-file | `cli/src/parity.ts` |
| 6 | LOW | **fixed** — the crash log rotates at 512 KiB to `<name>.old` (the previous file survives); tested by `the_log_file_stays_bounded`, which drives more entries than the cap and asserts the bound and the rotation | `kiln/src/reporting.rs` |
| 7 | LOW | **fixed** — `CHANGELOG.md` added (Keep a Changelog), README gained a "Compatibility policy" section naming what is and is not breaking | `CHANGELOG.md`, `README.md` |

Caveats kept honest: the Android CSP exception is documented but the underlying
wry/WebView behaviour was not re-measured on a device in this pass — the comment
in `security.rs` says exactly that and names the check that must precede any
tightening. Finding 2's fix is CI-only and proves itself the first time the
Windows and macOS jobs run; until then the e2e guarantee remains demonstrated on
Linux.
