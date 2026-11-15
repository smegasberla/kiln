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

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid]! : (sorted[mid - 1]! + sorted[mid]!) / 2;
}

async function sample(label: string, binary: string): Promise<number> {
  const values: number[] = [];
  for (let i = 0; i < RUNS; i++) {
    const value = await timeToReady(binary);
    if (value !== null) values.push(value);
    await Bun.sleep(150);
  }
  if (values.length === 0) throw new Error(`${label}: no READY line (binary failed to start)`);
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
console.log(
  `\n  overhead: ${(kiln - bare).toFixed(0)} ms (${delta >= 0 ? "+" : ""}${delta.toFixed(1)}%)`,
);
console.log(`  target: within +10% -> ${delta <= 10 ? "MET" : "MISSED"}`);

process.exit(delta <= 10 ? 0 : 1);
