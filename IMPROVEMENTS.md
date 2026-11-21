# Kiln — Improvement Backlog

Findings from the full-source review of the current tree (post `93caa52` fix pass,
which closed findings 1, 2, 4, 5, 6, 7 of `PRODUCTION-READINESS.md`). Each item
carries the evidence, why it matters, a fix direction, and an acceptance
criterion so it can be picked up and worked without re-reading the source.

Priority tiers: **T1** = high value, modest effort, start here. **T2** =
framework depth, needs a small design decision first. **T3** = polish. iOS is a
separate section (§9) because it is a platform project, not a code fix, and is
the user-selected focus.

---

## T1 — High value, modest effort

All five landed. Each item's acceptance criterion is asserted by a test, and the
typed-command rejection and the storage round-trip are also checked over the real
IPC transport by `scripts/verify.ts` on Linux, Windows and macOS.

### 1. Typed commands instead of `serde_json::Value` everywhere — **DONE**

**Evidence.** Every handler is `Fn(&AppHandle, Value) -> Result<Value, Error>`
(`kiln/src/ipc.rs`, `kiln/src/lib.rs` builder methods). Every example hand-rolls
`payload["name"].as_str().unwrap_or("world")` (`examples/counter`).

**Why it matters.** This is the biggest day-one friction for app authors: no
shape safety on either side of the bridge, so a wrong payload is discovered at
runtime as an `Error::Handler` string, not at compile time.

**Fix direction.** Add generic convenience methods that keep the existing raw
`Value` API as the base layer (no wire-format change):

```rust
app.command_typed("greet", |handle, p: GreetArgs| -> Result<Greet, Error> {
    Ok(Greet { hello: p.name })
})
```

Backed by `serde::de::DeserializeOwned` / `Serialize` (already in the tree via
`serde_json`). The generic wrapper deserializes the `Value`, calls the typed
body, serializes the result back into the envelope.

**Acceptance.** A typed command registers through the same allowlist and IPC
path; a payload of the wrong shape returns a structured error (see §3); at least
one example and the CLI template use the typed API; raw `Value` commands still
compile unchanged.

### 2. Async commands on a real async runtime, not raw threads — **DONE**

**Evidence.** `async_command` handlers spawn worker threads and bridge
sync/async with callbacks inside `ipc.rs`/`lib.rs`. There is no way to write an
`async fn` — the signature forces `Fn(...) -> Result<Value>` with manual thread +
channel plumbing for anything long-running.

**Why it matters.** Every real network/database command needs `async fn`. Forcing
thread-per-call makes authors reinvent executors, and under load a thread per
invocation is unbounded resource growth.

**Fix direction.** Add an `async_fn` registration variant behind an opt-in
cargo feature (e.g. `rt`, using `tokio` or a minimal embedded executor) so an
app can write:

```rust
app.async_fn("fetch_data", |_handle, p: Query| async move { ... })
```

The registry bridges the returned future to the existing envelope machinery —
the seam (`transport`, envelope) is already transport-agnostic and needs no
change. Measure dispatch latency before deciding on a thread pool for the
current path (see §14).

**Acceptance.** An `async fn` command returns through the same envelope; slow
async work does not block painting; the feature is off by default and the minimal
shell's binary size is unchanged without it.

### 3. Structured error kind on the wire — **DONE**

**Evidence.** The envelope is `{"ok": ...} | {"error": "..."}` with a plain
string. Internally the shell already distinguishes `Error::Handler`,
`Error::Runtime`, allowlist misses, and panics — but the page only sees text.

**Why it matters.** Frontends must string-match to tell "command not found" from
"handler panicked" from "bad payload". That is brittle and blocks sane frontend
error UI (retry vs. fatal vs. report).

**Fix direction.** Extend the envelope additively:

```json
{"error": {"message": "...", "kind": "panic|handler|payload|allowlist", "method": "..."}}
```

Readers keep accepting the current string form for back-compat; writers emit the
object. The variants already exist internally — this exposes them.

**Acceptance.** Every failure path (panic, handler `Err`, malformed payload,
allowlist deny, unknown method) emits a distinct `kind`; a test covers each; old
clients reading the string form are unaffected.

### 4. First-class lifecycle hooks on the builder — **DONE**

**Evidence.** `ReadyHook` exists but is internal plumbing; docs tell users to
"call from a handler or `on_ready`" without defining what `on_ready` is.
Android lifecycle states are already queued and flushed via `platform::page_loaded`
(`kiln/src/platform.rs:100-133`), but app authors have no documented hook to
subscribe to window-ready.

**Why it matters.** "Emit initial state to this window once it exists" is the
most common real-app need and currently requires knowing internals.

**Fix direction.** Promote `.on_ready(|app, window_label| ...)` and
`.on_window_close(|window_label| ...)` as documented builder methods, fired once
per window. Reuse the existing lifecycle queue for ordering guarantees.

**Acceptance.** An app can push initial state from `on_ready` without touching
internal APIs; a test asserts exactly-once per window; the API is in README and
rustdoc.

### 5. A documented data-persistence story — **DONE**

**Evidence.** `__kiln_data_dir` returns a path; docs say "null means no
persistence available" and stop there (`kiln/src/platform.rs:140-152`). No built-in
read/write, no companion crate in templates.

**Why it matters.** Nearly every desktop app needs settings persistence. Leaving
it entirely to the author makes Kiln feel unfinished versus Tauri's plugin
ecosystem, and every app reinvents the same JSON-file-in-data-dir code.

**Fix direction.** One minimal built-in: `storage.get/set/delete` backed by a JSON
file in the data dir (or a small `kiln-store` companion crate referenced in the
templates). Keep it feature-gated so the minimal shell stays minimal.

**Acceptance.** An app persists and reads a value in under ten lines; the feature
is opt-in; the data-dir null case degrades to an error the page can handle.

---

## T2 — Framework depth (design decision first)

### 6. Per-window allowlists (carried from PRODUCTION-READINESS.md finding 3)

**Evidence.** `kiln/src/config.rs` `commands` array narrows the compiled-in
handler set globally; `ipc.rs` applies one flat list. Built-ins
(`__kiln_open_window` etc.) are subject to the same list. Once the page can call
a command, any script in that page can.

**Why it matters.** Apps that render untrusted content (markdown previews,
embedded web content) inherit the full command surface. This is the wall Kiln
hits first as ambition grows.

**Fix direction.** `WindowOptions.allowlist: Option<Vec<String>>` intersecting the
global list, so a second window can be given a reduced surface. Full ACL is out
of scope — the minimal-shell promise forbids capability files.

**Acceptance.** A documented, tested path runs a second window with a strict
subset of the main window's commands.

### 7. Multi-window: labels are underused

**Evidence.** `__kiln_open_window` returns `Value::Null` — the page never learns
the label of the window it just created and cannot address it afterward.
`emit_to(Some(label))` works, but the label must be known a priori.

**Fix direction.** Return `{"label": "..."}` (generated when none given);
consider a `kiln.windows()` built-in listing open labels.

**Acceptance.** A page can open a window and immediately `emit_to` it; a test
asserts the returned label round-trips.

### 8. Tray/menu APIs are stringly-typed and fire-and-forget

**Evidence.** Menu items map to command names via `menu_command_map`; tray clicks
arrive as `{"id": id}` events (`kiln/src/platform.rs:55-67`). No runtime update,
no check-state, no native menu popup outside commands.

**Fix direction.** `TrayIconHandle`/`MenuHandle` with a few methods: set tooltip,
set checked, popup. Covers most real tray/menu apps.

**Acceptance.** A tray tooltip and a checked menu item can be updated at runtime
without re-registering commands.

### 9. Hot-reload state-loss: help rather than only document

**Evidence.** `kiln/src/hot.rs` honestly documents "statics do not survive reload;
persistent state travels in the payload" — nothing enforces or helps with it.
Watcher is a polling thread.

**Fix direction.** A shell-side `hot::state()` helper keyed by generation, so
reloading does not silently lose app state; a doc note on polling latency (or a
file-notify mechanism later).

**Acceptance.** A value written before a reload is readable after it, in a test.

### 10. Version the injected shim

**Evidence.** `window.kiln = {invoke, listen, platform}` is injected by
`build_shim` (`kiln/src/lib.rs:855-876`); `__kiln_version` reports the shell
version but the page cannot ask which shim features exist.

**Fix direction.** Add `kiln.version` (a shim protocol number) to the injected
object so frontend libraries feature-detect instead of branching on `platform`.

**Acceptance.** A page can read the shim protocol version; bumping it is a
documented change.

---

## T3 — Polish and operations

### 11. `kiln dev` structured diagnostics

Currently relies on eprintln + Bun dev output. A `--verbose` mode surfacing:
hot-lib generation on each swap, reload latency (already measured as
`built_at` in `hot.rs`), CSP/navigation denials in dev, command dispatch
latency.

### 12. Benchmark the async dispatch path

`cli/src/bench.ts` exists; measure async dispatch (thread-per-call vs. a pool)
before changing §2's implementation.

### 13. Documentation site (mdBook)

Everything lives in README + excellent inline rustdoc. A small mdBook ("Your
first app", "Commands & payloads", "Security model", "Multi-window",
"Packaging") generated from the same doc comments would make the framework feel
finished.

---

## §9. iOS support plan (user-selected focus)

**Current state (verified against the tree):**

- `platform_name()` already returns `"ios"` (`kiln/src/lib.rs:1074-1076`).
- `build_webview` already treats iOS like Windows/macOS/Android — direct attach,
  no GTK container (`kiln/src/lib.rs:830-843`).
- The transport seam already names iOS: "a mobile build replaces the transport
  (wry's IPC handler on Android, `WKScriptMessageHandler` on iOS)" (`lib.rs:877-880`).
- `kiln.d.ts` templates already declare `platform: ... | "ios"`.
- **Absent:** no `kiln-ios` crate, no entry point (iOS has no `fn main`), no
  `platform_data_dir` iOS branch (falls through to the XDG branch — wrong
  location inside the app sandbox), the shim's bridge flag is
  `cfg!(target_os = "android")` only (`lib.rs:862`), no packaging (`xcodebuild`,
  `.ipa`, signing), no CI job.
- README states "iOS is not supported" (`README.md:368`); PLAN.md marked it out of
  scope for v1.

**Plan — mirror the Android architecture, which already solved the same three
problems:**

**Step 1 — Transport (WKScriptMessageHandler).**
- `kiln-ios/` crate parallel to `kiln-android/`, or a `cfg(target_os = "ios")`
  module inside `kiln` following the `platform.rs` pattern — decide crate vs.
  module first; Android chose a separate crate because the activity owns the
  event loop, and iOS's `UIApplicationMain` does too, so a separate crate is the
  consistent choice.
- Reuse the existing `transport::Kind::MessageBridge` and the single
  envelope builder (`lib.rs` transport module) — no envelope drift.
- Extend `build_shim`'s bridge flag to `cfg!(any(target_os = "android",
  target_os = "ios"))`, and add the iOS origin to the allowlist the way
  `http://kiln.localhost` is added for Android (`lib.rs:866-869`).
- Use `WKURLSchemeHandler` for asset serving (wry already supports custom
  schemes on iOS) so the navigation allowlist and CSP apply identically to
desktop.

**Step 2 — Entry point and lifecycle.**
- Swift/ObjC bootstrap (`AppDelegate`) calls into Rust via a C FFI, mirroring
  the Android JNI activity hooks. `platform::set_app_handle` /
  `set_registry` / `page_loaded` / `lifecycle` are already public for exactly
  this (`platform.rs:24-133`) — iOS reports `didEnterBackground` /
  `willEnterForeground` through `platform::lifecycle`, which the existing
  LifecycleQueue already flushes to the page.

**Step 3 — Data dir.**
- Add a `cfg(target_os = "ios")` `platform_data_dir` branch returning the app
  sandbox's `Library/Application Support` (via `NSSearchPathForDirectoriesInDomains`
  or the already-known `HOME` inside the sandbox), replacing the accidental XDG
  fallthrough.

**Step 4 — CLI packaging.**
- `kiln ios build` / `kiln ios run`: bundle `dist/`, generate the Xcode project
  (or use a checked-in minimal one), `xcodebuild`, codesign with the developer's
  identity (mirror how `package.ts` explicitly does *not* attempt signing on
  desktop — iOS cannot skip it, so surface it as required config with a clear
  error).
- Parity check (`cli/src/parity.ts`) runs for iOS exactly as it does for Android
  — it already reads both shells' command registrations.

**Step 5 — CI.**
- The macOS runner already exists in `.github/workflows/ci.yml`. Add an iOS job:
  `cargo build --target aarch64-apple-ios` for compile coverage, plus
  `xcodebuild` on a simulator for at least the envelope/panic-containment e2e
  checks (same `scripts/verify.ts` guarantees, adapted to simulator). Full device
  e2e is out of CI scope; simulator covers the engine.

**Step 6 — Docs.**
- Flip `README.md:368` and the mobile row (`README.md:271`) to "iOS: experimental",
  document the signing requirement, and add a CHANGELOG entry.

**Acceptance for "iOS supported":** an app scaffolded by `kiln init` builds and
runs on the iOS simulator via `kiln ios run`, calls a command through the WK
bridge, survives a panic, and the CI job runs the e2e guarantees on WKWebView.

**Deliberate non-goals (same identity rules as desktop):** no JS runtime in the
shell, no plugin system, no CocoaPods/SPM dependency creep in the minimal path.

---

## What not to do (protect the framework's identity)

- **No plugin system.** "No plugin system, no capability files, no codegen" is
  the identity; §6's per-window allowlist is a config narrowing, not an ACL.
- **No JS runtime in the shell.** The ~785 KB binary is a headline number.
- **No UI/component library.** The frontend story is "bring your own."

---

## Suggested order

1. §1 typed commands + §3 structured errors (both small, additive, unblock every
   other item's DX).
2. §4 lifecycle hooks + §5 persistence (finish the day-one app-author story).
3. iOS plan §9 steps 1–3 (transport, entry, data dir) — the platform work the
   user asked to plan; steps 4–6 follow.
4. §6 per-window allowlist (decide scope, then implement).
5. §2 async runtime (after §12's measurement), then §7–§10, then T3.
