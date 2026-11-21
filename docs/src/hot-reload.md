# Hot reload in development

Hot reload is desktop-only and development-only. A release binary never loads a
dynamically linked command library; the same table that was hot-reloaded links
statically. That is checked, not asserted: `cargo tree -p voussoir --depth 1` shows no
`libloading` edge in a release build, and `scripts/verify-hot.ts` looks for the absence.

## What reloads

Two independent loops:

| Loop | Watches | Reloads |
| --- | --- | --- |
| frontend | everything the entry page imports | the page, via Bun HMR |
| commands | `native/commands/src/*.rs` | the command library, via a swap |

Changing a `.ts`, `.jsx` or `.css` file never touches Rust and never restarts the
shell. Changing `native/commands` rebuilds only that crate and swaps the resulting
library into the running process: no restart, no lost window state.

Changing `native/src/main.rs` does neither. The shell is the process; a change
there needs a restart, which is why the scaffold keeps app-level configuration
there and everything you iterate on in the commands crate.

## How the swap works

1. `voussoir dev` rebuilds `native/commands` and writes
   `target/voussoir-hot/commands-<n>.<ext>`.
2. It atomically renames a `latest.json` pointer at that file, including the
   timestamp the build started.
3. The shell polls the pointer every 250 ms, loads the new library, and swaps it in
   behind the registry.

A failed build publishes nothing, so the running shell keeps the last good library
and the compiler error stays on screen. That is the intended workflow: fix the
error, save, and the next successful build swaps in.

## The polling latency

The pointer is polled, not watched. A save therefore becomes visible up to 250 ms
after `cargo build` finishes, which is a quarter second on top of a build that is
usually a few hundred milliseconds. For a faster loop the cost is a file-notify
dependency and its platform quirks; the poll is cheap for a crate this size and
cannot miss a write-and-rename save an editor performs.

Each swap prints its rebuild-to-reload latency, and with `--verbose` it is also
recorded with its generation:

```
voussoir: [hot] t+8124ms hot commands gen 3 active gen=3 path=/…/commands-3.so rebuild_to_reload_ms=1533
```

## State across a reload

Statics inside the commands crate are rebuilt on every reload, because the library
that owned them is unloaded. State that must survive belongs to the shell:

```rust
#[cfg(feature = "hot-reload")]
fn bump() -> u64 {
    let n = voussoir::state::get("count").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
    voussoir::state::set("count", voussoir::serde_json::json!(n));
    n
}
```

`voussoir::state` is a process-wide `serde_json` map that the reload never touches. It
also works in a release build as scratch state, so code written against it does not
need two versions.

`voussoir::state::reload_generation()` reports which library generation is live. Use
it when a command must re-derive something once per generation, such as a connection
pool that should not be rebuilt on every call.

## What does not reload

- Rust release builds. There is no dynamic loading outside a dev feature.
- Mobile. Both platforms restrict loading dynamic code, so the loop does not exist
  there; `voussoir dev` is a desktop command.
- The shell crate. A change to `native/src/main.rs` needs a restart.

## Next

- [Diagnostics](diagnostics.md) — what `--verbose` records, and where to read it.
