/** Derive every identifier an app needs from one name.
 *
 *  Done in one place because these are easy to get subtly wrong, and a wrong
 *  package name produces a JNI symbol mismatch that shows up as
 *  `UnsatisfiedLinkError` at launch rather than as a build error.
 */

export type Identifiers = {
  /** Human-facing name, e.g. "My Notes". */
  display: string;
  /** Crate name and root directory slug, e.g. "my-notes". */
  crate: string;
  /** Rust `lib` name for the Android cdylib, e.g. "my_notes". Must be a valid identifier. */
  lib: string;
  /** Android package, e.g. "com.kiln.my_notes". */
  package: string;
  /** JNI mangling of the package, e.g. "com_kiln_my_1notes". */
  jniPackage: string;
  /** First segment of the JNI package for tao, e.g. "com". */
  domain: string;
  /** Last segment of the JNI package for tao, e.g. "my_notes". */
  leaf: string;
};

/** Slug for directories and crates: lowercase, hyphens, no leading digits. */
export function crateName(name: string): string {
  const base = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  const safe = base === "" ? "kiln-app" : base;
  return /^[0-9]/.test(safe) ? `app-${safe}` : safe;
}

/** Rust identifier: underscores, and never a keyword. */
export function libName(crate: string): string {
  const snake = crate.replace(/-/g, "_");
  const keywords = new Set([
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true",
    "type", "unsafe", "use", "where", "while", "async", "await", "dyn",
  ]);
  return keywords.has(snake) ? `${snake}_app` : snake;
}

/** A reverse-DNS package. `com.kiln.<slug>` by default, overridable by the user. */
export function packageName(crate: string, override?: string): string {
  if (override) return override;
  return `com.kiln.${libName(crate)}`;
}

/** JNI mangles `_` as `_1` and `.` as `_`. */
export function jniPackage(pkg: string): string {
  return pkg.replace(/_/g, "_1").replace(/\./g, "_");
}

export function identifiers(name: string, packageOverride?: string): Identifiers {
  const crate = crateName(name);
  const lib = libName(crate);
  const pkg = packageName(crate, packageOverride);
  const segments = pkg.split(".");
  return {
    display: name.trim() === "" ? "Kiln App" : name.trim(),
    crate,
    lib,
    package: pkg,
    jniPackage: jniPackage(pkg),
    domain: segments.slice(0, -1).join("_") || "com",
    leaf: segments[segments.length - 1] ?? lib,
  };
}
