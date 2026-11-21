# Operations

This section is the unglamorous half of a shipped app: what it costs to start,
what it tells you when something goes wrong, and how it behaves on each target.
None of it is needed to build a first screen, and all of it is what decides
whether a bug report is a five-minute fix or a week.

## Theory: why this is a section and not an appendix

Small projects tend to treat observability as something added after launch. That
ordering rarely works, because the decisions that make an app observable are
structural:

- whether the shell keeps a record of what happened (it does, in a ring buffer),
- whether a command runs on the UI thread or a worker (a per-command choice),
- whether a claim in the docs is measured or asserted (a per-claim choice).

Retrofitting any of those is a rewrite. Doing them from the start costs a table
row and a paragraph, which is what this section is.

## The three tools

Each is measured rather than asserted:

| Tool | Answers |
| --- | --- |
| `bun scripts/verify.ts` | does the shipped behaviour actually hold? |
| `bun scripts/bench.ts` | what does it cost to start, and per call? |
| `voussoir dev --verbose` | what happened in the last few seconds of this session? |

### `verify.ts`

Boots a real window on the real IPC path and asserts the things the book
guarantees:

- the page cannot run remote script, `eval`, or a remote `fetch`,
- navigation does not leave the app,
- a panicking handler produces an error envelope and the window survives,
- `storage` round-trips, and refuses past 4 MB,
- the multi-window contract: labels, listing, closing, and a narrowed window's
  denial,
- the release tree has no `libloading` edge and no JS runtime.

It is the source of the guarantees claimed elsewhere in this book. When a chapter
says "asserted", this is where.

### `bench.ts`

Cold start against a bare `wry`+`tao` control, and with `--dispatch` the cost of
the three execution models. See [Benchmarks](benchmarks.md).

### `--verbose`

Reloads, slow commands, refused navigations and panics, recorded so a snapshot
survives the terminal. See [Diagnostics](diagnostics.md).

## Theory: what "verified" means here

A claim in this book is either measured by one of those tools or it is not made.
The distinction shows up in the [Platform support](platforms.md) table, where a
cell says `cargo check` when the code compiles for the target but no window was
opened.

That vocabulary exists because "supported" is an ambiguous word, and the ambiguity
hides in the interesting place: a compile-only target is where runtime bugs live.
A table that marks it `yes` is not lying about effort, it is lying about
knowledge.

## Coverage

```bash
./scripts/coverage.sh
```

Reports line coverage for the library's own tests and fails under a floor. The
baseline is **67.58%**, measured with the default features and with test and fuzz
modules excluded, so the number describes the shell rather than its assertions.

## Theory: why a floor and not a target

Coverage is not a quality score. A fully covered function can be entirely wrong;
an uncovered one can be trivially correct. What coverage does measure is the thing
that decays quietly: **unexercised code paths accumulate**.

Three decisions make the gate useful rather than annoying:

1. **The floor sits a point or two under the measured number**, not at it. A gate
   set at the exact current value fails on honest changes and gets deleted.
2. **Test and fuzz modules are excluded from the denominator**, because counting
   assertions as covered code inflates the number and hides the gap.
3. **Raising is a decision; lowering is not.** Raise when a release closes a real
   gap. Never lower it to make a build pass, because that converts a measurement
   into a decoration.

It uses the `llvm-profdata` and `llvm-cov` the toolchain already links against, so
there is nothing to install and CI runs the same script as a developer.

## Theory: what an exit code is worth

Every tool here fails with a non-zero exit when its claim does not hold. That is
what lets CI be a review rather than a ritual: the build is green because the
claims were checked, not because nobody looked.

The corollary is a rule worth adopting in your own app: **a test that cannot fail
is documentation**. If removing the implementation still passes, the test is
asserting that the process did not crash.

## In this section

- [Diagnostics](diagnostics.md) — the `--verbose` mode and what it records.
- [Benchmarks](benchmarks.md) — the numbers and how to reproduce them.

## Next

- [Troubleshooting](troubleshooting.md) — symptom to cause, by layer.
- [Testing](testing.md) — writing the assertions this section relies on.
