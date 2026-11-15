/** `kiln init [dir]`: write a project. Templates are inlined into the CLI
 *  binary, so an installed `kiln` has no asset directory to lose. */

import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { kilnDependency } from "../framework.ts";
import { boolFlag, stringFlag, type Args } from "../args.ts";

import kilnJson from "../../template/kiln.json.tmpl" with { type: "text" };
import cargoToml from "../../template/Cargo.toml.tmpl" with { type: "text" };
import mainRs from "../../template/main.rs.tmpl" with { type: "text" };
import assetsRs from "../../template/assets.rs.tmpl" with { type: "text" };
import indexHtml from "../../template/index.html.tmpl" with { type: "text" };
import mainTs from "../../template/main.ts.tmpl" with { type: "text" };
import styleCss from "../../template/style.css.tmpl" with { type: "text" };
import tsconfig from "../../template/tsconfig.json.tmpl" with { type: "text" };
import kilnDts from "../../template/kiln.d.ts.tmpl" with { type: "text" };
import gitignore from "../../template/gitignore.tmpl" with { type: "text" };

export function init(args: Args): number {
  const dir = resolve(args.positionals[0] ?? ".");
  const force = boolFlag(args, "force");
  const name = stringFlag(args, "name") ?? humanize(basename(dir));

  if (existsSync(dir) && readdirSync(dir).length > 0 && !force) {
    console.error(`kiln: ${dir} is not empty (pass --force to write anyway)`);
    return 1;
  }

  const crate = crateName(basename(dir));
  const fill = (template: string) =>
    template
      .replaceAll("{{NAME}}", name)
      .replaceAll("{{CRATE}}", crate)
      .replaceAll("{{KILN_PATH}}", kilnDependency())
      .replaceAll("{{KILN_PATH_INNER}}", kilnDependency());

  const write = (path: string, contents: string) => {
    const full = join(dir, path);
    mkdirSync(join(full, ".."), { recursive: true });
    writeFileSync(full, contents);
    console.log(`  created ${path}`);
  };

  console.log(`\nScaffolding ${name} in ${dir}\n`);
  write("kiln.json", fill(kilnJson));
  write("index.html", fill(indexHtml));
  write("src/main.ts", fill(mainTs));
  write("src/style.css", fill(styleCss));
  write("src/kiln.d.ts", kilnDts);
  write("tsconfig.json", fill(tsconfig));
  write(".gitignore", gitignore);
  write("native/Cargo.toml", fill(cargoToml));
  write("native/src/main.rs", fill(mainRs));
  write("native/src/assets.rs", assetsRs);

  console.log(`\nNext:\n  cd ${args.positionals[0] ?? "."}\n  kiln dev\n`);
  return 0;
}

function humanize(slug: string): string {
  const base = slug === "." || slug === "" ? "kiln-app" : slug;
  return base
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase())
    .trim();
}

function crateName(slug: string): string {
  const base = slug === "." || slug === "" ? "kiln-app" : slug;
  return base
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}
