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

/** Names registered with `.command("x", ..)`, `.async_command("x", ..)`,
 *  `.command_typed("x", ..)` or `.async_fn("x", ..)`.
 *
 *  Comments are stripped first: a commented-out registration is not a
 *  registration, and treating one as real would hide a genuine gap. */
export function registeredCommands(source: string): string[] {
  const names = new Set<string>();
  const pattern = /\.(?:async_)?(?:command(?:_typed)?|fn)\(\s*"([^"]+)"/g;
  for (const match of stripComments(source).matchAll(pattern)) names.add(match[1]!);
  return [...names].sort();
}

/** Registrations whose name this parser cannot read.
 *
 *  `registeredCommands` matches a bare string literal, so a name assembled at
 *  runtime (`concat!`, a const array walked in a loop, a variable) is invisible to
 *  it. That is worse than a mismatch: the command exists, parity is unverified for
 *  it, and nothing says so. Counting them turns silence into a line to read.
 *
 *  KNOWN LIMITATIONS — this is a regex over source text, not a parser:
 *  - dynamic names are counted here and never resolved, so two shells can still
 *    disagree about them while parity passes;
 *  - a string literal that contains `.command("x"` is reported as a registration;
 *  - `include!` and `mod` are followed exactly one level deep.
 *  A pass therefore means "no statically visible mismatch", never "parity". */
export function unreadableRegistrations(source: string): string[] {
  const stripped = stripComments(source);
  const calls = /\.(?:async_)?(?:command(?:_typed)?|fn)\s*\(\s*([^,)\n]*)/g;
  const names: string[] = [];
  for (const match of stripped.matchAll(calls)) {
    const argument = match[1]!.trim();
    if (!/^"[^"]+"$/.test(argument)) names.push(argument || "<empty>");
  }
  return names;
}

/** Remove `//` and block comments without disturbing string literals enough to
 *  matter here: command names never contain `//` or `/*`. */
function stripComments(source: string): string {
  return stripTestModules(
    source
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/^\s*\/\/.*$/gm, ""),
  );
}

/** Remove `#[cfg(test)] mod x { … }` blocks, with balanced braces.
 *
 *  A test-only table's `name: "boom"` literals are not runtime registrations,
 *  but the name-literal regex above cannot tell them apart from the real table
 *  — and the scaffold ships exactly such a test module, so every fresh scaffold
 *  would fail parity on commands that do not exist in release. A regex cannot
 *  balance braces, so this walks the text: an attribute that is followed by a
 *  `mod` before its first brace marks a block, and the scan skips to its
 *  matching brace. */
function stripTestModules(source: string): string {
  let out = "";
  let i = 0;
  const attribute = "#[cfg(test)]";
  while (i < source.length) {
    const start = source.indexOf(attribute, i);
    if (start === -1) return out + source.slice(i);
    const open = source.indexOf("{", start);
    if (open === -1 || !/\bmod\b/.test(source.slice(start, open))) {
      // Not a module: keep the attribute and look for the next one.
      out += source.slice(i, start + attribute.length);
      i = start + attribute.length;
      continue;
    }
    let depth = 0;
    let end = open;
    for (; end < source.length; end++) {
      if (source[end] === "{") depth++;
      else if (source[end] === "}") {
        depth--;
        if (depth === 0) break;
      }
    }
    out += source.slice(i, start);
    i = depth === 0 ? end + 1 : open;
  }
  return out;
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

/** Names in a commands-crate table: `Command { name: "x", .. }`.
 *
 *  The scaffolded desktop shell registers its commands by walking that table,
 *  so the literals live in the commands crate rather than in `.command(…)`
 *  calls in `main.rs`. Only pass files known to contain command tables. */
export function tableCommands(source: string): string[] {
  const names = new Set<string>();
  const pattern = /\bname:\s*"([^"]+)"/g;
  for (const match of stripComments(source).matchAll(pattern)) names.add(match[1]!);
  return [...names].sort();
}

/** Table entries whose name this parser cannot read — the `tableCommands`
 *  counterpart of `unreadableRegistrations`, same contract: a dynamic name is
 *  counted, never resolved, so silence never hides an unverified command. */
export function unreadableTableRegistrations(source: string): string[] {
  const found: string[] = [];
  const entries = /Command\s*\{\s*name:\s*([^,\n]+)/g;
  for (const match of stripComments(source).matchAll(entries)) {
    const raw = match[1]!.trim();
    if (!raw.startsWith('"')) found.push(raw);
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

/** Read the crates and report the difference, ignoring a missing Android or
 *  iOS crate.
 *
 *  The iOS crate is only included in the comparison when it exists, so a
 *  desktop+Android project behaves exactly as it did before Phase I-3; the iOS
 *  shell joins the parity set the moment `kiln init` writes one. */
export function checkParity(root: string): { ok: boolean; message: string } {
  const desktopFile = join(root, "native", "src", "main.rs");
  // The commands crate is where the scaffolded desktop shell keeps its
  // registration table since commands became hot-reloadable.
  const commandsFile = join(root, "native", "commands", "src", "lib.rs");
  const androidFile = join(root, "android", "src", "lib.rs");
  const iosFile = join(root, "ios", "src", "lib.rs");

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

  const desktopSource = readFileSync(desktopFile, "utf8");
  const androidSource = readFileSync(androidFile, "utf8");
  const desktop = new Set(effectiveCommands(desktopSource, join(root, "native", "src")));
  let unreadable = unreadableRegistrations(desktopSource).length;
  if (existsSync(commandsFile)) {
    const commandsSource = readFileSync(commandsFile, "utf8");
    for (const name of tableCommands(commandsSource)) desktop.add(name);
    unreadable += unreadableTableRegistrations(commandsSource).length;
  }
  const android = effectiveCommands(androidSource, join(root, "android", "src"));
  unreadable += unreadableRegistrations(androidSource).length;

  let ios: string[] = [];
  const platforms = ["desktop", "android"];
  if (existsSync(iosFile)) {
    const iosSource = readFileSync(iosFile, "utf8");
    ios = effectiveCommands(iosSource, join(root, "ios", "src"));
    unreadable += unreadableRegistrations(iosSource).length;
    platforms.push("iOS");
  }
  const { onlyDesktop, onlyAndroid } = diffCommands([...desktop].sort(), android);
  const onlyIos = ios.filter((name) => !desktop.has(name));
  const missingIos = ios.length === 0 ? [] : [...desktop].filter((name) => !ios.includes(name));
  const caveat = unreadable > 0 ? ` (${unreadable} registration(s) not statically readable)` : "";

  if (
    onlyDesktop.length === 0 &&
    onlyAndroid.length === 0 &&
    onlyIos.length === 0 &&
    missingIos.length === 0
  ) {
    return { ok: true, message: `${desktop.size} commands on ${platforms.join(", ")}${caveat}` };
  }

  const parts = [
    onlyDesktop.length ? `desktop only: ${onlyDesktop.join(", ")}` : "",
    onlyAndroid.length ? `android only: ${onlyAndroid.join(", ")}` : "",
    onlyIos.length ? `iOS only: ${onlyIos.join(", ")}` : "",
    missingIos.length ? `desktop+android missing on iOS: ${missingIos.join(", ")}` : "",
  ].filter(Boolean);

  return { ok: false, message: `${parts.join("; ")}${caveat}` };
}
