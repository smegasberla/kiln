/** Locating the Kiln crates: workspace-relative when hacking on the framework,
 *  published versions when installed. */

import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The dependency body written into a scaffolded `Cargo.toml`.
 *
 *  Returned without the surrounding braces so a template can add features. */
export function kilnDependency(): string {
  const found = locate("kiln");
  if (found) return `path = "${found}"`;
  const override = process.env.KILN_PATH;
  if (override) return `path = "${resolve(override).replace(/\\/g, "/")}"`;
  return 'version = "0.1"';
}

/** The Android bootstrap crate, resolved the same way. */
export function kilnAndroidDependency(): string {
  const found = locate("kiln-android");
  if (found) return `path = "${found}"`;
  return 'path = "../kiln/kiln-android"';
}

/** Absolute path to a sibling crate in this workspace, or null when installed. */
function locate(crate: string): string | null {
  const cliDir = dirname(fileURLToPath(import.meta.url));
  for (const candidate of [
    join(cliDir, "..", ".."),
    join(cliDir, "..", "..", ".."),
    join(cliDir, "..", "..", "..", ".."),
  ]) {
    if (existsSync(join(candidate, crate, "Cargo.toml"))) {
      return resolve(join(candidate, crate)).replace(/\\/g, "/");
    }
  }
  return null;
}
