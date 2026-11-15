/** `kiln doctor`: environment and project checks, without spawning a build.
 *
 *  Every check reads something real (a binary on PATH, a file, a manifest) so a
 *  green report means the next command will actually work.
 */

import { existsSync } from "node:fs";
import { join } from "node:path";
import { boolFlag, type Args } from "../args.ts";
import { findProject } from "../project.ts";

type Check = { name: string; ok: boolean; detail: string };

const BUN_VERSION = /^(\d+)\.(\d+)\./;

export async function doctor(args: Args): Promise<number> {
  const checks: Check[] = [];

  const bun = Bun.which("bun");
  checks.push({
    name: "bun",
    ok: Boolean(bun) && (Bun.version.match(BUN_VERSION)?.[1] ?? "0") >= "1",
    detail: bun ? `${bun} (${Bun.version})` : "not found on PATH",
  });

  const cargo = Bun.which("cargo");
  checks.push({
    name: "cargo",
    ok: Boolean(cargo),
    detail: cargo ?? "not found on PATH; install Rust",
  });

  const webkit = process.platform !== "linux" || hasWebkit();
  checks.push({
    name: "system webview",
    ok: webkit,
    detail: webkit ? "available" : "webkit2gtk-4.1 not found (install webkit2gtk-4.1)",
  });

  const project = findProject();
  checks.push({
    name: "project",
    ok: Boolean(project),
    detail: project ? project.root : "no kiln.json in this directory or a parent",
  });

  if (project) {
    const entry = join(project.root, project.config.entry);
    checks.push({
      name: "entry html",
      ok: existsSync(entry),
      detail: existsSync(entry) ? project.config.entry : `missing: ${entry}`,
    });
    const manifest = join(project.root, "native", "Cargo.toml");
    checks.push({
      name: "native crate",
      ok: existsSync(manifest),
      detail: existsSync(manifest) ? "native/Cargo.toml" : "missing: native/Cargo.toml",
    });
  }

  if (boolFlag(args, "json")) {
    console.log(JSON.stringify({ ok: checks.every((c) => c.ok), checks }, null, 2));
  } else {
    for (const check of checks) {
      console.log(`${check.ok ? "ok  " : "FAIL"} ${check.name.padEnd(14)} ${check.detail}`);
    }
  }

  return checks.every((c) => c.ok) ? 0 : 1;
}

function hasWebkit(): boolean {
  const pkgconfig = Bun.which("pkg-config");
  if (!pkgconfig) return false;
  const proc = Bun.spawnSync(["pkg-config", "--exists", "webkit2gtk-4.1"]);
  if (proc.exitCode === 0) return true;
  const legacy = Bun.spawnSync(["pkg-config", "--exists", "webkit2gtk-4.0"]);
  return legacy.exitCode === 0;
}
