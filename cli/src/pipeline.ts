/** The one place that knows how a Kiln project turns into a Rust binary.
 *
 *  `kiln dev` and `kiln build` differ in exactly two things: whether the
 *  frontend is bundled to disk or served by Bun, and the cargo profile. Both
 *  funnel through `writeAssetTable` so the Rust side is identical either way.
 */

import { readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { assetTable } from "./assets.ts";
import { crateDir, requireProject, type Project } from "./project.ts";
import { run } from "./proc.ts";

export const OUT_DIR = "dist";

/** Bundle the frontend to `dist/`. Bun handles TS, JSX, CSS, and HTML entries. */
export async function bundle(project: Project, minify: boolean): Promise<void> {
  const outDir = join(project.root, OUT_DIR);
  rmSync(outDir, { recursive: true, force: true });
  const args = ["build", join(project.root, project.config.entry), "--outdir", OUT_DIR, "--target", "browser"];
  if (minify) args.push("--minify");
  await run(["bun", ...args], { cwd: project.root });
}

/** Regenerate `native/src/assets.rs` from the current `dist/`. */
export function writeAssetTable(project: Project): number {
  const crate = crateDir(project);
  const outDir = join(project.root, OUT_DIR);
  const target = join(crate, "src", "assets.rs");
  const { entries, source } = assetTable(outDir, join(crate, "src"));
  writeFileSync(target, source);
  return entries;
}

/** Fail early with a useful message instead of a wall of cargo errors. */
export function ensureToolchain(): void {
  for (const bin of ["bun", "cargo"]) {
    if (!Bun.which(bin)) {
      throw new Error(`\`${bin}\` is not on PATH; install it first`);
    }
  }
}

export function project(): Project {
  const p = requireProject();
  if (!Bun.which("cargo")) throw new Error("`cargo` is not on PATH; install Rust first");
  return p;
}

/** Build the Rust binary, optionally with the release profile. */
export async function cargoBuild(project: Project, release: boolean): Promise<string> {
  const crate = crateDir(project);
  const args = ["cargo", "build"];
  if (release) args.push("--release");
  await run(args, { cwd: crate });
  const bin = join(crate, "target", release ? "release" : "debug", binaryName(crate));
  if (!statSync(bin, { throwIfNoEntry: false })) {
    throw new Error(`cargo finished but ${bin} is missing`);
  }
  return bin;
}

/** The crate's binary name, read from Cargo.toml to avoid guessing. */
function binaryName(crate: string): string {
  const manifest = readFileSync(join(crate, "Cargo.toml"), "utf8");
  const named = /\[\[bin\]\][\s\S]*?name\s*=\s*"([^"]+)"/.exec(manifest);
  if (named) return named[1]!;
  const pkg = /\[package\][\s\S]*?name\s*=\s*"([^"]+)"/.exec(manifest);
  if (!pkg) throw new Error("could not read package name from native/Cargo.toml");
  return pkg[1]!;
}
