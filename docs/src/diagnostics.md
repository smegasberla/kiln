# Diagnostics

`voussoir dev` normally prints only what you would read: the dev server URL, a
successful rebuild, a failed compile. `voussoir dev --verbose` adds four things that
are otherwise invisible, and records them all so a snapshot survives the terminal.

```bash
voussoir dev --verbose
```

## What it records

| Kind | Recorded when | Useful for |
| --- | --- | --- |
| `hot` | a command library generation goes live | "did my edit actually reach the app?" |
| `dispatch` | a command takes 50 ms or more | which command makes the window hitch |
| `security` | navigation is refused by the policy | "why did that link do nothing?" |
| `bridge` | a bridge message is dropped | a page calling `postMessage` with the wrong shape |
| `app` | a handler panicked | a crash in the field |

Each entry carries a monotonic sequence number, milliseconds since process start,
the message, and a JSON object of fields. Printing is one line per entry, so a log
can be grepped:

```
voussoir: [hot] t+8124ms hot commands gen 3 active gen=3 path=/…/commands-3.so rebuild_to_reload_ms=1533
voussoir: [dispatch] t+9010ms slow command method=read_file ms=412
voussoir: [security] t+10233ms navigation refused dev_origin=null url=https://example.com/
```

## Reading it from the app

The buffer is also reachable from the page, which is what makes it useful on a
platform with no terminal (Android, iOS):

```ts
const { verbose, entries } = await window.voussoir.invoke("__voussoir_diagnostics", { tail: 20 });
console.table(entries);
```

`tail` is optional; omit it for everything in the buffer. The buffer holds the last
256 entries and drops the oldest, so a dev session that runs for hours does not
grow the process.

`__voussoir_diagnostics_enabled` answers only the flag, for a diagnostics screen that
wants to show the state without fetching the entries.

## Latency on exit

When the app closes, `--verbose` prints a per-command latency table, slowest mean
first:

```
voussoir: command latency, slowest first:
  read_file                    p50 410 ms  p95 512 ms  max 610 ms  n 12
  greet                        p50   0 ms  p95   1 ms  max   3 ms  n 48
```

The numbers are measured around the handler, not around the transport, so they
answer "is my command slow" rather than "is the webview slow". Samples are capped at
the last 1000 calls per command, and nothing is recorded at all when `--verbose` is
off, so a release build pays nothing: the check is one cached atomic load.

## Why recorded and not just printed

A printed line is gone once it scrolls past, and there is no scrollback at all on a
phone. The buffer makes the last few reloads and denials inspectable after the fact,
which is the difference between "it sometimes freezes" and "read_file took 610 ms
twelve times".

## Enabling it without the CLI

`voussoir dev --verbose` sets `VOUSSOIR_VERBOSE=1` for the shell it launches. The shell
honours the variable directly, so a `cargo run` in a debug build gets the same
output. `VOUSSOIR_VERBOSE=0` forces it off even if the variable is inherited from a
shell profile.
