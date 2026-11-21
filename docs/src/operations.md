# Operations

This section is about the unglamorous half of a shipped app: what it costs to
start, what it tells you when something goes wrong, and how it behaves on each
target. None of it is required to build a first screen, and all of it is what
decides whether a bug report is a five-minute fix or a week.

Three tools exist for it, and each is measured rather than asserted:

- `bun scripts/verify.ts` boots a real window on the real IPC path and asserts the
  security policy, the panic boundary, the storage round-trip and the multi-window
  contract. It is the source of the guarantees claimed elsewhere in this book.
- `bun scripts/bench.ts` measures cold start against a bare `wry`+`tao` binary, and
  with `--dispatch` measures the async dispatch path.
- `kiln dev --verbose` records reload, dispatch and denial events while you work.

## What "verified" means here

A claim in this book is either measured by one of those tools or it is not made. The
table in [Platform support](platforms.md) marks a cell `cargo check` when the code
compiles for the target but no window was opened, and says so rather than implying
runtime coverage.

## In this section

- [Diagnostics](diagnostics.md) — the `--verbose` mode and what it records.
- [Benchmarks](benchmarks.md) — the numbers and how to reproduce them.

## Coverage

`./scripts/coverage.sh` reports line coverage for the library's own tests and
fails under a floor. The baseline is **67.58%**, measured with the default
features and with test and fuzz modules excluded, so the number describes the
shell rather than its assertions. The floor sits just under it: a gate set at the
exact current number fails on honest changes and gets deleted, while one a point
below still catches code added without a test.

It uses the `llvm-profdata` and `llvm-cov` the toolchain already links against,
so there is nothing to install and CI runs the same script. Raise the floor when a
release closes a real gap; never lower it to make a build pass.
