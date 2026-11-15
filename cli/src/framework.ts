/** Locating the `kiln` runtime crate: workspace-relative when hacking on the
 *  framework, a crates.io version when installed. */

import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The dependency body written into a scaffolded `Cargo.toml`.
 *
 *  Returned without the surrounding braces so the template can add features. */
export function kilnDependency(): string {
  const override = process.env.KILN_PATH;
  if (override) return `path = "${resolve(override).replace(/\\/g, "/")}"`;

  const cliDir = dirname(fileURLToPath(import.meta.url));
  for (const candidate of [join(cliDir, "..", ".."), join(cliDir, "..", "..", "..")]) {
    const manifest = join(candidate, "kiln", "Cargo.toml");
    if (existsSync(manifest)) {
      return `path = "${resolve(join(candidate, "kiln")).replace(/\\/g, "/")}"`;
    }
  }
  return 'version = "0.1"';
}
