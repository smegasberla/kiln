/** Command parity between the desktop and Android crates.
 *
 *  A Kiln app has one frontend and two shells. If the frontend calls a command
 *  that only one shell registers, the other platform fails at runtime with
 *  "unknown command" — which is exactly the kind of bug that ships, because both
 *  apps build cleanly and only the platform nobody tested complains.
 *
 *  This reads the registered names out of the two source files and compares them.
 */

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

/** Names registered with `.command("x", ..)` or `.async_command("x", ..)`.
 *
 *  Comments are stripped first: a commented-out registration is not a
 *  registration, and treating one as real would hide a genuine gap. */
export function registeredCommands(source: string): string[] {
  const names = new Set<string>();
  const pattern = /\.(?:async_)?command\(\s*"([^"]+)"/g;
  for (const match of stripComments(source).matchAll(pattern)) names.add(match[1]!);
  return [...names].sort();
}

/** Remove `//` and block comments without disturbing string literals enough to
 *  matter here: command names never contain `//` or `/*`. */
function stripComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "");
}

/** Commands one platform has that the other does not. */
export function diffCommands(
  desktop: string[],
  android: string[],
): { onlyDesktop: string[]; onlyAndroid: string[] } {
  const d = new Set(desktop);
  const a = new Set(android);
  return {
    onlyDesktop: desktop.filter((name) => !a.has(name)),
    onlyAndroid: android.filter((name) => !d.has(name)),
  };
}

/** Files a crate pulls in with `#[path = ".."] mod x;` or `include!("..")`.
 *
 *  An app that shares one command list between its two shells has no list of its
 *  own to drift, and reading only the crate root would report "0 commands on both
 *  platforms" for it: true, but useless. Following the include is what makes the
 *  count mean something there. */
export function includedFiles(source: string, crateDir: string): string[] {
  const stripped = stripComments(source);
  const found: string[] = [];
  const patterns = [
    /#\[path\s*=\s*"([^"]+)"\]\s*(?:pub\s+)?mod\s+\w+\s*;/g,
    /include!\s*\(\s*"([^"]+)"\s*\)/g,
  ];
  for (const pattern of patterns) {
    for (const match of stripped.matchAll(pattern)) {
      found.push(resolve(crateDir, match[1]!));
    }
  }
  // A plain `mod app;` is the same sharing, done the ordinary way: the module is
  // `app.rs` or `app/mod.rs` beside the file that declares it.
  for (const match of stripped.matchAll(/^\s*(?:pub\s+)?mod\s+(\w+)\s*;/gm)) {
    const name = match[1]!;
    for (const candidate of [`${name}.rs`, `${name}/mod.rs`]) {
      const path = resolve(crateDir, candidate);
      if (existsSync(path)) {
        found.push(path);
        break;
      }
    }
  }
  return found;
}

/** Every command a crate registers, following one level of shared includes. */
export function effectiveCommands(source: string, crateDir: string): string[] {
  const names = new Set(registeredCommands(source));
  for (const file of includedFiles(source, crateDir)) {
    if (existsSync(file)) {
      for (const name of registeredCommands(readFileSync(file, "utf8"))) names.add(name);
    }
  }
  return [...names].sort();
}

/** Read both crates and report the difference, ignoring a missing Android crate. */
export function checkParity(root: string): { ok: boolean; message: string } {
  const desktopFile = join(root, "native", "src", "main.rs");
  const androidFile = join(root, "android", "src", "lib.rs");

  if (!existsSync(desktopFile)) {
    if (existsSync(androidFile)) {
      // `kiln init --mobile` writes no desktop crate: there is no second command
      // list to disagree with, so parity is trivially satisfied.
      return { ok: true, message: "mobile only; skipping parity" };
    }
    return { ok: false, message: `missing ${desktopFile}` };
  }
  if (!existsSync(androidFile)) {
    // A desktop-only project is legitimate.
    return { ok: true, message: "no android crate; skipping parity" };
  }

  const desktop = effectiveCommands(
    readFileSync(desktopFile, "utf8"),
    join(root, "native", "src"),
  );
  const android = effectiveCommands(
    readFileSync(androidFile, "utf8"),
    join(root, "android", "src"),
  );
  const { onlyDesktop, onlyAndroid } = diffCommands(desktop, android);

  if (onlyDesktop.length === 0 && onlyAndroid.length === 0) {
    return { ok: true, message: `${desktop.length} commands on both platforms` };
  }

  const parts = [
    onlyDesktop.length ? `desktop only: ${onlyDesktop.join(", ")}` : "",
    onlyAndroid.length ? `android only: ${onlyAndroid.join(", ")}` : "",
  ].filter(Boolean);

  return { ok: false, message: parts.join("; ") };
}
