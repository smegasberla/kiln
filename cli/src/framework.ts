/** Locating the Voussoir crates: workspace-relative when hacking on the framework,
 *  published versions when installed. */

import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The dependency body written into a scaffolded `Cargo.toml`.
 *
 *  Returned without the surrounding braces so a template can add features. */
export function voussoirDependency(): string {
  const found = locate("voussoir");
  if (found) return `path = "${found}"`;
  const override = process.env.VOUSSOIR_PATH;
  if (override) return `path = "${resolve(override).replace(/\\/g, "/")}"`;
  return 'version = "0.1"';
}

/** The Android bootstrap crate, resolved the same way. */
export function voussoirAndroidDependency(): string {
  const found = locate("voussoir-android");
  if (found) return `path = "${found}"`;
  return 'path = "../voussoir/voussoir-android"';
}

/** The iOS bootstrap crate, resolved the same way. */
export function voussoirIosDependency(): string {
  const found = locate("voussoir-ios");
  if (found) return `path = "${found}"`;
  return 'path = "../voussoir/voussoir-ios"';
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
