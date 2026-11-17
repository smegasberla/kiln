# PLAN — Hot-Reloadable Commands (cdylib + libloading)

Feature plan only. The original project plan lives in git history
(`git show 93caa52:PLAN.md`).

Goal: `kiln dev` recompiles and swaps command code into the running app without
restarting the process or the webview. Release builds keep compiling commands
statically into the single shell binary, with zero dependency or size change.

## Non-negotiable constraints (restated as acceptance rules)

1. Release builds unaffected: static linking, no `libloading`, same shell size.
2. No new dependency in the release tree. `libloading` is behind a cargo
   feature that `kiln build --release` never enables.
3. Desktop only. Android/iOS out of scope for v1 (dynamic loading is
   restricted on mobile).
4. A panic in a hot-reloaded command must not kill the host (envelope error,
   window survives), exactly like the static path.

## Architecture decisions (decided once, here)

**One source of truth.** `native/commands/` is a single crate, always built as
`crate-type = ["rlib", "cdylib"]`. Release links the rlib statically; dev
loads the cdylib. No dev/release fork of command code.

**Commands that move, and the one class that cannot.** Payload-in/payload-out
commands (`greet`, `slow_task`, …) live in the commands crate. A command that
needs `AppHandle` (`open_settings`) cannot cross the dylib boundary: a Rust
handle type from the host is not the same type inside the dylib, so passing it
would be unsound. Shell-service commands stay in `native/src/main.rs` and are
registered statically in both modes. This is a documented v1 boundary, not an
accident.

**ABI contract (v1, fixed).** Three C symbols, no per-command symbols:

```text
kiln_dispatch(method: *const c_char, payload: *const c_char) -> *mut c_char
    UTF-8 in, UTF-8 out; returns the standard envelope {"ok":…}|{"error":…}.
    Body catches all panics (catch_unwind) so nothing unwinds across the
    dylib boundary; the host wraps the call in catch_unwind again (belt).
kiln_free(ptr: *mut c_char)
    Frees a pointer returned by kiln_dispatch (cross-allocator safety).
kiln_methods() -> *const c_char
    JSON array of the command names this build serves, so the host can list
    hot commands diagnostically and distinguish "removed" from "unknown".
```

**Dispatch precedence.** Static handlers registered in `main.rs` are consulted
first; a miss falls through to the hot library when one is attached. In dev,
`main.rs` registers only shell-service commands, so payload commands resolve
through the hot path. A name removed from the commands crate resolves to
"unknown command" after the next reload (no stale static copy: the table is
only registered statically when the `hot-reload` feature is off).

**Swap safety.** New dylibs are copied to `target/kiln-hot/commands-<gen>.<ext>`
(temp + rename) and pointed at by `latest.json`. The shell polls that file on a
background thread (std only, no watcher dependency). Replaced libraries are
never unloaded — kept alive for the life of the dev session; correctness over
reclamation, dev-only. Failed load keeps the previous library and prints a loud
error.

**State rule (documented, not solved).** Statics inside the commands crate do
not survive a reload. Persistent state must be owned by the shell and passed
into each call. No state-migration system in v1.

**Fast rebuild assumption.** The commands crate depends on `serde_json` only
(no `kiln`, no wry/tao/gtk), so `cargo build -p commands` is the scoped
rebuild. Actual rebuild-to-reload latency is measured and reported every
reload (built timestamp in `latest.json`, shell prints total).

## Phases, in order

### Phase 1 — Commands crate split (static behavior preserved)
- Template: `native/commands/{Cargo.toml,src/lib.rs}` with the command table
  (`greet`, `slow_task`) and a `commands()` accessor; `native/Cargo.toml`
  becomes the workspace root (`members = ["commands"]`).
- `main.rs.tmpl`: registers the table (sync/async per entry) + keeps
  `open_settings` locally, when `hot-reload` is off.
- `init.ts` writes the new files; parity checker learns to read the commands
  crate alongside `main.rs` so desktop/Android command names still match.
- **Done when:** `kiln init` → `kiln build` produces a working app; parity and
  all CLI tests pass; `cargo test -p kiln` unaffected.

### Phase 2 — ABI boundary in the commands crate
- `kiln_dispatch` / `kiln_free` / `kiln_methods` exports, panic caught inside,
  envelope produced by the same shape the static path uses.
- **Done when:** commands-crate unit tests cover: happy dispatch, unknown
  method, malformed payload, and a panicking command returning
  `{"error":…}` instead of unwinding.

### Phase 3 — Shell: feature, swap, registry routing
- `kiln` gains feature `hot-reload = ["dep:libloading"]` (never part of
  `full`), module `hot.rs`: load, generation swap under `RwLock`, keep-alive
  list, poll thread on `KILN_HOT_DIR/latest.json`, `call()` wrapped in
  `catch_unwind`.
- `Registry` gains an optional hot slot; `dispatch_deferred` falls through to
  it after static handlers, still behind the config allowlist.
- `App::hot_commands_from_env()` (reads `KILN_HOT_LIB`, `KILN_HOT_DIR`;
  loud message when the feature is on but the env is absent).
- **Done when:** unit tests cover swap-with-test-`extern "C"` functions,
  panic containment through `call()`, allowlist still narrowing hot methods,
  envelope validation of a bad library reply; clippy `-D warnings` clean.

### Phase 4 — CLI: watcher, scoped rebuild, signal
- `dev.ts` builds the commands crate once up front, copies gen 1, spawns the
  shell with `--features hot-reload` and the env vars; watches
  `native/commands/src`, on change runs `cargo build -p commands`, copies to
  `commands-<gen>.<ext>` + `latest.json` with `builtAt`; build failure keeps
  last-good and prints the compiler error.
- **Done when:** manual loop proven: edit → rebuild → shell logs
  `reloaded gen N` with total latency; shell PID unchanged; syntax error →
  old behavior retained, error shown.

### Phase 5 — Verification harness
- `scripts/verify-hot.ts`: scaffolds an app, drives `kiln dev` headless
  (xvfb), page reports each `greet` reply + a monotonically increasing
  page-side counter over HTTP to the harness. Asserts: reply changes after an
  edit while the counter never resets (no process/webview restart); panic
  command returns an error envelope and the next call still works; syntax
  error keeps last-good and reports; the release shell has no direct
  `libloading` edge (`cargo tree --depth 1`) and the minimal shell size
  check still passes; reports rebuild-to-reload latency.
- Wired into the Linux CI job (where xvfb already runs). Windows/macOS e2e for
  this feature is deferred (see below); unit tests are platform-neutral.
- **Done when:** `bun scripts/verify-hot.ts` passes locally and in CI.

### Phase 6 — Docs
- README: "Hot reload (dev only)" section — ABI contract, state-does-not-
  persist rule, desktop-only statement, how to read the latency report.
- The commands crate template carries the same contract as module docs.
- CHANGELOG entry under Unreleased.
- **Done when:** docs match behavior (each claim maps to a verified check).

### Phase 7 — Full suite, commit, push
- `cargo test -p kiln --all-features`, `cargo clippy --all-features -D
  warnings`, `bun test`, `bun run typecheck`, `bun scripts/verify.ts`,
  `bun scripts/verify-hot.ts` all green; commit; push.

## Explicitly deferred

- Android/iOS hot reload (out of scope by constraint).
- Windows/macOS hot-reload e2e in CI (logic is parameterized; harness gated to
  the Linux job for v1).
- Unloading old dylibs (kept for session life on purpose).
- State migration across reloads (documented rule instead).
- Unify the ABI with a future `#[command]` macro: no such macro exists in this
  repo today; the boundary is documented as the shape one would generate.
- Serve hot commands to the parity checker through a structural (non-regex)
  model — tracked in the review remediation plan instead.
