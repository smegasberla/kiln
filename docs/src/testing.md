# Testing

Voussoir apps are tested at three levels, and knowing which level a problem
belongs to is most of the skill. This chapter covers all three, plus the traps
specific to an app whose UI is a webview.

## Theory: what is testable, and what is not

The parts of an app are not equally testable, and pretending otherwise produces
slow, flaky tests that get deleted:

| Layer | Testable without a window | How |
| --- | --- | --- |
| Command logic | yes | plain Rust unit tests |
| Payload parsing and errors | yes | plain Rust unit tests |
| The registry and allowlist | yes | `Registry` in unit tests |
| The shim's JS | yes | run it under Node/Bun with a fake bridge |
| The real IPC transport | no | needs a webview: `scripts/verify.ts` |
| Rendering and CSS | no | a human, or a screenshot diff |

The rule that follows: **put logic where the first four rows can reach it.** A
command that is a thin wrapper around a well-tested function is worth more than a
command that contains the logic, because the logic can then be tested in
milliseconds.

## Level 1: Rust unit tests

Commands are ordinary functions, so they test like ordinary functions:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use voussoir::serde_json::json;

    #[test]
    fn greet_uses_the_supplied_name() {
        let reply = greet(&json!({ "name": "ada" })).unwrap();
        assert_eq!(reply, json!("Hello, ada!"));
    }

    #[test]
    fn greet_falls_back_when_the_name_is_missing() {
        assert_eq!(greet(&json!({})).unwrap(), json!("Hello, world!"));
    }

    #[test]
    fn greet_ignores_a_name_that_is_not_a_string() {
        assert_eq!(greet(&json!({ "name": 7 })).unwrap(), json!("Hello, world!"));
    }
}
```

Run them with:

```bash
cargo test                        # the whole workspace
cargo test -p commands            # just your command crate
cargo test greet_uses             # one test, by substring
```

The third test is the one people skip and then regret: a page is untrusted input
like any other, and `{"name": 7}` is a real thing a bug will send.

**What to cover:** every failure branch, and one success path per command. Do not
aim for 100%: aim for "a wrong reply fails a test" for the commands that touch
files, money, or the user's data.

## Level 2: the registry

Whether a command is reachable at all is worth testing, because a missing table
entry is a silent `unknown` in production:

```rust
#[test]
fn every_command_in_the_table_is_registered() {
    let app = yourapp::build_app();   // a helper that returns the configured App
    for command in voussoir::commands::COMMANDS {
        assert!(app.has_command(command.name), "{} is not registered", command.name);
    }
}
```

`has_command` answers for the registry alone and ignores the allowlist, so this
asserts "exists", not "granted". The allowlist is a separate test:

```rust
#[test]
fn a_preview_window_cannot_reach_delete() {
    let registry = /* build with commands ["greet"] and windows {preview: ["greet"]} */;
    assert_eq!(registry.resolve("preview", "delete"), None);
}
```

The `ipc` unit tests in the crate cover the intersection rule directly, if you
want a worked example.

## Level 3: the real webview

Some things can only be proven with a window: that the shim installed, that the
custom protocol answered, that a panic became an `{error}` envelope instead of
killing the process, that navigation was actually refused.

`scripts/verify.ts` does exactly this. It boots a real window against a real
page and asserts:

- a command round-trip over the real transport,
- a hostile page's script does not execute,
- `eval` and remote `fetch` are blocked,
- navigation away from the app is refused,
- a panicking handler returns `kind: "panic"` and the window survives,
- the storage round-trip works,
- a narrowed window is denied.

Run it locally:

```bash
bun scripts/verify.ts
```

It needs a display. On a headless Linux box, `xvfb-run -a bun scripts/verify.ts`
is what CI uses.

**Why not unit-test those?** Because the thing being tested is the webview's
behaviour, not yours. A mock would assert that your mock works.

## Testing the page

The shim is deliberately replaceable, which makes page code testable without a
window:

```ts
// A test double that matches the shape the shim installs.
function fakeVoussoir(replies: Record<string, unknown>) {
  return {
    invoke: async (method: string) => {
      if (!(method in replies)) {
        throw Object.assign(new Error("unknown"), { kind: "unknown", method });
      }
      return replies[method];
    },
    listen: () => () => {},
    windows: async () => [],
    platform: "linux" as const,
    pageOrigin: "voussoir://localhost",
    protocol: 3,
    timeout: 30000,
  };
}

beforeEach(() => {
  (globalThis as any).window = { voussoir: fakeVoussoir({ greet: "hi" }) };
});
```

Keep the double in one file. When the shim gains a member, that file is the only
place to update, and a missing member shows up as a type error rather than a
mystery `undefined` in one test.

## What CI runs

| Job | Command | Catches |
| --- | --- | --- |
| Rust tests | `cargo test` | logic, registry, panic boundary |
| CLI tests | `bun test` in `cli/` | scaffolder, docs check, dev server helpers |
| End to end | `bun scripts/verify.ts` | the real transport, policy, panic path |
| Hot reload | `bun scripts/verify-hot.ts` | the swap, and no `libloading` in release |
| Coverage | `./scripts/coverage.sh` | untested code, against a floor |
| Docs | `voussoir docs --check` | stale links and summary drift |

Run the first three before opening a pull request. The rest are cheap enough to
let CI do, but a red coverage job is worth reading: it usually means a new branch
has no test.

## The traps

- **A green test that never ran the handler.** If you test through the registry,
  assert the reply, not just that no error came back.
- **Timing in a webview test.** "Wait 100 ms for the page to load" is a flake
  waiting to happen. Wait for a signal the page sends (an `invoke` that only
  happens after load) instead.
- **Testing the panic path with `catch_unwind` in your own code.** You cannot:
  the shell owns the boundary. Assert it end to end.
- **Snapshotting HTML.** It breaks on whitespace and teaches nobody anything.
  Assert the text a user reads.

## Next

- [Benchmarks](benchmarks.md) — measuring what tests cannot assert.
- [Diagnostics](diagnostics.md) — what to record when a test cannot help.
