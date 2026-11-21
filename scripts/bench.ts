/** Cold-start comparison: bare wry+tao vs the Kiln shell.
 *
 *  Time is measured from process start to the page-load-finished callback, in
 *  a release binary, repeated and reported as a median. Both binaries do the
 *  same amount of work: create an event loop, a window, a webview, and load a
 *  page.
 *
 *  Run: bun scripts/bench.ts
 */

import { join, resolve } from "node:path";

const repo = resolve(import.meta.dir, "..");
const RUNS = 7;
let dispatchFailed = false;

// `--dispatch` measures the async dispatch path instead of cold start, and answers
// the question §2 of the plan asks: is a task-per-call runtime worth its
// dependency, or is thread-per-call good enough? The probe binary prints raw
// measurements; the assertions live here so the gate is in one place.
if (process.argv.includes("--dispatch")) {
  await dispatch();
  process.exit(dispatchFailed ? 1 : 0);
}

async function timeToReady(binary: string, cwd = repo): Promise<number | null> {
  const proc = Bun.spawn([binary], {
    cwd,
    env: { ...process.env, KILN_BENCH: "1" },
    stdout: "pipe",
    stderr: "ignore",
  });
  const timer = setTimeout(() => proc.kill(), 30_000);
  const out = await new Response(proc.stdout).text();
  clearTimeout(timer);
  const match = /READY (\d+)/.exec(out);
  return match ? Number(match[1]) : null;
}

/** Build the dispatch benchmark, run it, and assert what it reports. */
async function dispatch(): Promise<void> {
  const bin = join(repo, "target", "release", `dispatch${process.platform === "win32" ? ".exe" : ""}`);
  const build = Bun.spawn(["cargo", "build", "-p", "kiln-probe", "--bin", "dispatch", "--release"], {
    cwd: repo,
    stdout: "pipe",
    stderr: "inherit",
  });
  if ((await build.exited) !== 0) {
    console.log("FAIL dispatch benchmark did not build");
    dispatchFailed = true;
    return;
  }

  const proc = Bun.spawn([bin], {
    cwd: repo,
    env: { ...process.env, KILN_BENCH_CALLS: "300" },
    stdout: "pipe",
    stderr: "inherit",
  });
  const output = await new Response(proc.stdout).text();
  if ((await proc.exited) !== 0) {
    console.log("FAIL dispatch benchmark exited non-zero");
    dispatchFailed = true;
    return;
  }

  const json = (tag: string) => {
    const match = new RegExp(`^${tag} (.+)$`, "m").exec(output);
    return match ? JSON.parse(match[1]!) : null;
  };
  const meta = json("BENCH-META");
  const decision = json("BENCH-DECISION");
  const crossover = json("BENCH-CROSSOVER");
  const overheads: Record<string, number> = {};
  for (const match of output.matchAll(/^BENCH-OVERHEAD (.+)$/gm)) {
    const row = JSON.parse(match[1]!);
    overheads[row.method] = row.per_call_us;
  }

  const rows = [...output.matchAll(/^BENCH (.+)$/gm)].map((match) => JSON.parse(match[1]!));
  for (const row of rows) {
    console.log(
      `  ${String(row.method).padEnd(7)} c=${String(row.concurrency).padStart(3)}  ` +
        `${row.per_call_ms.toFixed(3)} ms/call`,
    );
  }

  const record = (name: string, ok: boolean, detail: string) => {
    if (!ok) dispatchFailed = true;
    console.log(`${ok ? "ok  " : "FAIL"} ${name.padEnd(34)} ${detail}`);
  };

  // A benchmark that ran the wrong thing is worse than none: assert the shape
  // before asserting the conclusion.
  record(
    "benchmark ran every path",
    meta !== null && rows.length > 0 && ["sync", "thread", "pool"].every((m) => overheads[m] !== undefined),
    meta ? `${meta.calls} calls, levels ${meta.levels.join("/")}, work ${meta.work_ms} ms` : "no output",
  );
  // No work at all: dispatch overhead alone. The sync path must be the cheapest,
  // or the registry itself is doing something surprising.
  record(
    "sync dispatch is the cheapest path",
    overheads.sync !== undefined && overheads.sync < Math.min(overheads.thread ?? Infinity, overheads.pool ?? Infinity),
    `sync ${overheads.sync?.toFixed(1)} us vs thread ${overheads.thread?.toFixed(0)} / pool ${overheads.pool?.toFixed(0)} us`,
  );
  // The decision §2 asks for. Not "the pool is always faster": it must win where a
  // pool is for, and the crossover is reported so the number is not a claim.
  record(
    "the pool wins where concurrency exists",
    decision !== null && decision.pooled_ms < decision.thread_per_call_ms,
    decision
      ? `at ${decision.concurrency}: pool ${decision.pooled_ms.toFixed(3)} ms vs thread ${decision.thread_per_call_ms.toFixed(3)} ms (${decision.ratio.toFixed(2)}x)`
      : "no decision line",
  );
  record(
    "crossover is reported",
    crossover !== null && Number.isInteger(crossover.concurrency),
    crossover ? `first level where the pool wins: ${crossover.concurrency}` : "no crossover line",
  );
  // The runtime is a dependency, so it must not be slower sequentially either. It
  // is allowed to be close: a sleeping thread and a timer entry both cost about
  // the sleep itself.
  const sequential = (method: string) =>
    rows.find((row) => row.method === method && row.concurrency === 1)?.per_call_ms ?? Infinity;
  record(
    "the pool is competitive sequentially",
    sequential("pool") < sequential("thread") * 1.5,
    `pool ${sequential("pool").toFixed(3)} ms vs thread ${sequential("thread").toFixed(3)} ms per call at concurrency 1`,
  );
}

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid]! : (sorted[mid - 1]! + sorted[mid]!) / 2;
}

const samples: Record<string, number[]> = { kiln: [], bare: [] };

async function sample(label: string, binary: string): Promise<number> {
  const values: number[] = [];
  for (let i = 0; i < RUNS; i++) {
    const value = await timeToReady(binary);
    if (value !== null) values.push(value);
    await Bun.sleep(150);
  }
  if (values.length === 0) throw new Error(`${label}: no READY line (binary failed to start)`);
  samples[label === "kiln shell" ? "kiln" : "bare"] = values;
  const med = median(values);
  console.log(
    `  ${label.padEnd(22)} median ${String(med).padStart(4)} ms   ` +
      `[${values.join(", ")}]`,
  );
  return med;
}

const bare = await sample("bare wry+tao", join(repo, "target", "release", "bare"));
const kiln = await sample("kiln shell", join(repo, "target", "release", "examples", "hello"));

const delta = ((kiln - bare) / bare) * 100;

// A sample far above the median means the machine was busy (a concurrent build,
// a browser, another benchmark), not that the shell got slower. Say so instead of
// reporting a regression that is really measurement noise.
function spread(values: number[]): number {
  const med = median(values);
  return Math.max(...values) / med;
}

if (spread(samples.kiln) > 1.6 || spread(samples.bare) > 1.6) {
  console.log(
    "\n  noisy run: at least one sample was far above the median. Close other\n" +
      "  work and run again before trusting this number.",
  );
}

console.log(
  `\n  overhead: ${(kiln - bare).toFixed(0)} ms (${delta >= 0 ? "+" : ""}${delta.toFixed(1)}%)`,
);
console.log(`  target: within +10% -> ${delta <= 10 ? "MET" : "MISSED"}`);

process.exit(delta <= 10 ? 0 : 1);
