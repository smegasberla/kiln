/** `kiln build`: bundle the frontend, embed it, compile the shell. */

import { join } from "node:path";
import {
  bundle,
  cargoBuild,
  ensureToolchain,
  project,
  writeAssetTable,
} from "../pipeline.ts";
import { boolFlag, stringFlag, type Args } from "../args.ts";

export async function build(args: Args): Promise<number> {
  ensureToolchain();
  const p = project();
  const release = !boolFlag(args, "debug");

  console.log(`kiln build: ${p.config.name}`);
  await bundle(p, true);
  const count = writeAssetTable(p);
  console.log(`  embedded ${count} frontend file(s)`);

  const binary = await cargoBuild(p, release);
  console.log(`\n  ${binary}`);

  if (boolFlag(args, "size")) {
    const { size } = Bun.file(binary);
    console.log(`  ${(size / 1024 / 1024).toFixed(2)} MB`);
  }

  const out = stringFlag(args, "out");
  if (out) {
    await Bun.write(join(process.cwd(), out), Bun.file(binary));
    console.log(`  copied to ${out}`);
  }

  return 0;
}
