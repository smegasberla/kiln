# Benchmarks

Two numbers matter, and both are measured on the machine you run them on rather
than quoted from a spec sheet.

## Cold start

```bash
bun scripts/bench.ts
```

Times process start to the page-load-finished callback, in a release binary,
against a bare `wry`+`tao` binary that does the same work: one event loop, one
window, one webview, one page. Seven samples each, reported as a median.

The script also reports if a sample is far above the median, which means the
machine was busy rather than the shell being slow. It exits non-zero if the
overhead exceeds +10%, so CI can gate on it.

## Async dispatch

```bash
bun scripts/bench.ts --dispatch
```

Measures the three execution models against each other:

| Path | Model | Cost per call |
| --- | --- | --- |
| `command` | UI thread | a function call |
| `async_command` | thread per call | a thread spawn and join |
| `async_fn` | task on a shared runtime | a task submit |

The interesting comparison is the last two, because that is the decision an app
author actually makes. `async_command` spawns a thread per call, which is fine for
a handful of concurrent calls and wasteful for a thousand. `async_fn` submits a
task, which costs a task rather than a stack.

The benchmark exists to keep that claim honest: it prints the per-call cost of each
model at several concurrency levels, so "async_fn scales better" is a number rather
than an opinion. It also fails if the pooled path is ever slower than the
thread-per-call path at the concurrency level where it is supposed to win, which
would mean the runtime is misconfigured.

## What is not benchmarked

- Startup on Windows and macOS. The binary is built and tested there, but the
  cold-start number in the README is Linux, and says so.
- Render performance. That is the webview's job, and measuring it here would measure
  the platform rather than Voussoir.
- Binary size over time. `verify.ts` checks the size bound; it is a gate, not a
  trend.
