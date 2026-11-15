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
import { join } from "node:path";

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

/** Read both crates and report the difference, ignoring a missing Android crate. */
export function checkParity(root: string): { ok: boolean; message: string } {
  const desktopFile = join(root, "native", "src", "main.rs");
  const androidFile = join(root, "android", "src", "lib.rs");

  if (!existsSync(desktopFile)) {
    return { ok: false, message: `missing ${desktopFile}` };
  }
  if (!existsSync(androidFile)) {
    // A desktop-only project is legitimate.
    return { ok: true, message: "no android crate; skipping parity" };
  }

  const desktop = registeredCommands(readFileSync(desktopFile, "utf8"));
  const android = registeredCommands(readFileSync(androidFile, "utf8"));
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
