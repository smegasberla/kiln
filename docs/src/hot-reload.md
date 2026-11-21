# Hot reload in development

Hot reload is desktop-only and development-only. A release binary never loads a
dynamically linked command library; the same table that was hot-reloaded links
statically.

That is checked, not asserted: `cargo tree -p voussoir --depth 1` shows no
`libloading` edge in a release build, and `scripts/verify-hot.ts` looks for the
absence.

## Theory: why dynamic loading at all, and why only in dev

Recompiling a webview shell takes a minute. Recompiling a crate of plain Rust
functions takes a second. Hot reload exploits that gap: `voussoir dev` builds your
command crate as a shared library, and the running shell loads the new one when it
appears.

The obvious worry is that this introduces dynamic loading into what is supposed to
be a single static binary. So the mechanism is fenced:

| Configuration | Command crate built as | Who loads it |
| --- | --- | --- |
| `voussoir dev` | `cdylib` | the shell, at runtime |
| `voussoir build` | static library | the linker |

The hot-reload code is behind a cargo feature (`hot-reload`) that only
`voussoir dev` enables. A release binary does not contain it, which is why the
`libloading` check can be a hard assertion rather than a guideline.

## What reloads

Two independent loops:

| Loop | Watches | Reloads |
| --- | --- | --- |
| frontend | everything the entry page imports | the page, via Bun HMR |
| commands | `native/commands/src/*.rs` | the command library, via a swap |

Changing a `.ts`, `.jsx` or `.css` file never touches Rust and never restarts the
shell. Changing `native/commands` rebuilds only that crate and swaps the resulting
library into the running process: no restart, no lost window state.

Changing `native/src/main.rs` does neither. The shell *is* the process; a change
there needs a restart, which is why the scaffold keeps app-level configuration
there and everything you iterate on in the commands crate.

## How the swap works

1. `voussoir dev` rebuilds `native/commands` and writes
   `target/voussoir-hot/commands-<n>.<ext>`.
2. It atomically renames a `latest.json` pointer at that file, including the
   timestamp the build started.
3. The shell polls the pointer every 250 ms, loads the new library, and swaps it in
   behind the registry.

**Why a pointer file and not just overwrite the library?** Because the running
process has the old one mapped. Overwriting a mapped file is undefined behaviour
on some platforms; writing a new file and moving a pointer is atomic everywhere.
The timestamp in the pointer also lets a swap be ignored if it is older than the
generation already loaded, so two builds finishing out of order cannot regress the
shell.

A failed build publishes nothing, so the running shell keeps the last good library
and the compiler error stays on screen. That is the intended workflow: fix the
error, save, and the next successful build swaps in.

## The polling latency

The pointer is polled, not watched. A save therefore becomes visible up to 250 ms
after `cargo build` finishes, which is a quarter second on top of a build that is
usually a few hundred milliseconds.

**Why not a file watcher?** A watcher costs a dependency (`notify`), a set of
platform quirks, and a class of bugs where an editor's write-and-rename pattern
produces two events or none. The poll is cheap for a crate this size and cannot
miss a rename.

Each swap prints its rebuild-to-reload latency, and with `--verbose` it is also
recorded with its generation:

```
voussoir: [hot] t+8124ms hot commands gen 3 active gen=3 path=/…/commands-3.so rebuild_to_reload_ms=1533
```

## Theory: state across a reload

Statics inside the commands crate are rebuilt on every reload, because the library
that owned them is unloaded. This is the single most surprising consequence of the
design, so it is worth stating plainly:

```rust
// This counter resets every time you save. It compiles, it "works", and it is wrong.
static CALLS: AtomicU64 = AtomicU64::new(0);
```

State that must survive belongs to the shell:

```rust
fn bump() -> u64 {
    let n = voussoir::state::get("count").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
    voussoir::state::set("count", voussoir::serde_json::json!(n));
    n
}
```

`voussoir::state` is a process-wide JSON map that the reload never touches. It also
works in a release build as scratch state, so code written against it does not
need two versions. See [Storage](storage.md) if you need it to survive a restart,
not just a reload.

`voussoir::state::reload_generation()` reports which library generation is live.
Use it when a command must re-derive something once per generation:

```rust
static POOL_GEN: AtomicU64 = AtomicU64::new(0);

fn pool() -> &'static Pool {
    let gen = voussoir::state::reload_generation();
    // Rebuild the pool when the generation changed, not on every call.
    // ...
}
```

A connection pool is the worked example: it should not be rebuilt on every call,
but it must be rebuilt when the code that built it changed.

## What does not reload

- **Rust release builds.** There is no dynamic loading outside the dev feature.
- **Mobile.** Both platforms restrict loading dynamic code, so the loop does not
  exist there; `voussoir dev` is a desktop command.
- **The shell crate.** A change to `native/src/main.rs` needs a restart.
- **`voussoir.json`.** It is `include_str!`-embedded, so a change needs a rebuild
  of the shell, not just a reload.
- **The frontend build config.** `index.html` and its imports are re-bundled by
  Bun; a change to how they are bundled is a restart.

## When a swap fails

| Symptom | Cause |
| --- | --- |
| edits never appear | the build is failing; read the cargo error |
| a command still answers with old behaviour | it lives in `main.rs`, not the table |
| counters reset on save | a `static` in the commands crate |
| the first save after start is slow | the initial `cdylib` build |

The last one is normal: the first build of the commands crate has nothing cached.

## Next

- [How a project is laid out](layout.md) — why the crate is separate.
- [Testing](testing.md) — `scripts/verify-hot.ts` in CI.
