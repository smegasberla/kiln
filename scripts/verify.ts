/** End-to-end verification for the repo, and the source of the README numbers.
 *
 *  Everything here asserts something a human would otherwise check by looking
 *  at a window: the shell boots, the JS -> Rust -> JS round-trip returns real
 *  data, `emit` reaches the page, the binary is small, and the binary contains
 *  no JS runtime.
 *
 *  Run: bun scripts/verify.ts
 */

import { mkdtempSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dir, "..");
const cli = join(repo, "cli", "src", "index.ts");
const results: { name: string; ok: boolean; detail: string }[] = [];

function record(name: string, ok: boolean, detail: string) {
  results.push({ name, ok, detail });
  console.log(`${ok ? "ok  " : "FAIL"} ${name.padEnd(30)} ${detail}`);
}

async function sh(cmd: string[], cwd = repo, env: Record<string, string> = {}) {
  const proc = Bun.spawn(cmd, {
    cwd,
    env: { ...process.env, ...env },
    stdout: "pipe",
    stderr: "pipe",
  });
  const [out, err, code] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  return { out, err, code };
}

async function shWithTimeout(cmd: string[], cwd: string, env: Record<string, string>, ms: number) {
  const proc = Bun.spawn(cmd, {
    cwd,
    env: { ...process.env, ...env },
    stdout: "pipe",
    stderr: "pipe",
  });
  const timer = setTimeout(() => proc.kill(), ms);
  const [out, err, code] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  clearTimeout(timer);
  return { out, err, code };
}

/** The page a probe loads. It calls the app's own `greet` command over the real
 *  IPC path and POSTs the response back so this script can assert on it. */
const PAGE = `<!doctype html><html><body><script>
(async () => {
  try {
    const res = await fetch('kiln://localhost/__kiln/greet', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ name: 'scaffold' }),
    });
    await fetch('/result', { method: 'POST', body: await res.text() });
  } catch (e) {
    await fetch('/result', { method: 'POST', body: JSON.stringify({ error: String(e) }) });
  }
})();
</script></body></html>`;

/** Serve PAGE on `port`, point the shell at it, and return what the page
 *  reported. The page origin differs from the shell's origin, which is exactly
 *  the cross-origin path `kiln dev` depends on. */
async function probe(binary: string, app: string, port: number): Promise<string | null> {
  let resolveReply: (value: string) => void = () => {};
  const reply = new Promise<string>((r) => (resolveReply = r));

  const server = Bun.serve({
    port,
    async fetch(req) {
      const url = new URL(req.url);
      if (url.pathname === "/result" && req.method === "POST") {
        resolveReply(await req.text());
        return new Response("ok");
      }
      if (url.pathname === "/" || url.pathname === "/index.html") {
        return new Response(PAGE, { headers: { "content-type": "text/html" } });
      }
      return new Response("not found", { status: 404 });
    },
  });

  const proc = Bun.spawn([binary], {
    cwd: app,
    env: { ...process.env, KILN_DEV_URL: `http://127.0.0.1:${port}` },
    stdout: "ignore",
    stderr: "ignore",
  });

  const body = await Promise.race([
    reply,
    new Promise<null>((r) => setTimeout(() => r(null), 25_000)),
  ]);

  proc.kill();
  server.stop(true);
  if (body === null) return null;
  try {
    const parsed = JSON.parse(body) as { ok?: string; error?: string };
    return parsed.ok ?? parsed.error ?? null;
  } catch {
    return body;
  }
}

async function main() {
  // 1. Shell unit tests (dispatch, assets, config, protocol handling).
  {
    const { out, code } = await sh(["cargo", "test", "-p", "kiln", "--quiet"]);
    const passed = /test result: ok\. (\d+) passed/.exec(out.replace(/\s+/g, " "))?.[1] ?? "0";
    record("shell unit tests", code === 0, `${passed} tests`);
  }

  // 2. Embedded-asset path: the framework example boots, round-trips a command
  //    over kiln://, and receives a Rust -> JS event. It exits 0 only when both
  //    arrive, so the exit code is the assertion.
  {
    const { out, err, code } = await shWithTimeout(
      [join(repo, "target", "release", "examples", "hello")],
      repo,
      { KILN_SELFTEST: "1" },
      30_000,
    );
    record(
      "embedded round-trip + emit",
      code === 0 && out.includes("SELFTEST OK"),
      code === 0 ? out.trim().split("\n").pop()! : err.slice(-120),
    );
  }

  // 3. Scaffold a fresh app with the CLI and build it with the CLI.
  const work = mkdtempSync(join(tmpdir(), "kiln-verify-"));
  const app = join(work, "app");
  {
    const { code, err } = await sh(["bun", cli, "init", app]);
    record("kiln init", code === 0, code === 0 ? app : err.slice(-120));
  }
  {
    const { out, err, code } = await shWithTimeout(
      ["bun", cli, "build", "--size"],
      app,
      {},
      600_000,
    );
    record(
      "kiln build",
      code === 0,
      code === 0 ? out.trim().split("\n").slice(-2).join(" ") : err.slice(-160),
    );
  }

  const binary = join(app, "native", "target", "release", "app");

  // 4. Size and composition of the shipped artifact.
  {
    const size = statSync(binary).size;
    record("binary size", size < 4 * 1024 * 1024, `${(size / 1048576).toFixed(2)} MB`);
  }
  {
    const bytes = readFileSync(binary);
    const markers = ["Bun.version", "node_modules", "electron", "Chromium"];
    const found = markers.filter((m) => bytes.includes(Buffer.from(m)));
    record("no JS runtime embedded", found.length === 0, found.join(",") || "clean");
  }
  {
    const { out } = await sh(["ldd", binary]);
    const node = /libnode|libv8/.test(out);
    const webkit = /libwebkit2gtk/.test(out);
    record(
      "links OS webview only",
      !node && (process.platform !== "linux" || webkit),
      node ? "links libnode/libv8" : "webkit2gtk present, no node/v8",
    );
  }

  // 5. The scaffolded binary opens a window with its embedded page and stays
  //    up. A crash, a missing asset, or a bad protocol registration would exit
  //    early; so surviving two seconds is the signal.
  {
    const proc = Bun.spawn([binary], { cwd: app, stdout: "pipe", stderr: "pipe" });
    const exited = await Promise.race([
      proc.exited.then((code) => String(code)),
      Bun.sleep(2000).then(() => "alive" as const),
    ]);
    proc.kill();
    record(
      "embedded app launches",
      exited === "alive",
      exited === "alive" ? "window up after 2s" : `exited early (${exited})`,
    );
  }

  // 6. Dev mode on the same binary: page from http, IPC still over kiln://.
  {
    const debug = await sh(["bun", cli, "build", "--debug"], app);
    if (debug.code !== 0) {
      record("dev-mode round-trip", false, "debug build failed");
    } else {
      const reported = await probe(join(app, "native", "target", "debug", "app"), app, 5192);
      record(
        "dev-mode round-trip",
        reported !== null && reported.includes("from Rust"),
        reported ?? "no reply",
      );
    }
  }

  // 7. The distributed CLI is a compiled binary: it must work with no Bun or
  //    Node on PATH, and produce the same app a `bun run` CLI does.
  {
    const out = join(work, "kiln-bin");
    const compiled = await sh(["bun", "build", "--compile", "--minify", cli, "--outfile", out]);
    if (compiled.code !== 0) {
      record("compiled CLI", false, compiled.err.slice(-140));
    } else {
      const bare = join(work, "cli-only-air");
      const env = { PATH: "/usr/bin:/bin", HOME: bare };
      const init = await sh([out, "init", join(bare, "app")], work, env);
      const size = (statSync(out).size / 1048576).toFixed(0);
      record(
        "compiled CLI (no bun on PATH)",
        init.code === 0,
        init.code === 0 ? `${size} MB binary, init ok` : init.err.slice(-140),
      );
    }
  }

  const failed = results.filter((r) => !r.ok);
  console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
  process.exit(failed.length === 0 ? 0 : 1);
}

await main();
