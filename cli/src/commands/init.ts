/** `kiln init [dir]`: write a project.
 *
 *  Templates are inlined into the CLI binary at compile time, so an installed
 *  `kiln` has no asset directory to lose, and a scaffolded project contains no
 *  generated code: everything under `android/` is either configuration you may
 *  edit or source you own.
 */

import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { kilnAndroidDependency, kilnDependency } from "../framework.ts";
import { boolFlag, stringFlag, type Args } from "../args.ts";
import { identifiers } from "../naming.ts";

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

import androidBuildGradle from "../../template/android/build.gradle.tmpl" with { type: "text" };
import androidSettings from "../../template/android/settings.gradle.tmpl" with { type: "text" };
import androidProps from "../../template/android/gradle.properties.tmpl" with { type: "text" };
import androidManifest from "../../template/android/AndroidManifest.xml.tmpl" with { type: "text" };
import androidThemes from "../../template/android/themes.xml.tmpl" with { type: "text" };
import androidActivity from "../../template/android/MainActivity.java.tmpl" with { type: "text" };
import androidCargo from "../../template/android/Cargo.toml.tmpl" with { type: "text" };
import androidLib from "../../template/android/lib.rs.tmpl" with { type: "text" };
import androidAssets from "../../template/android/assets.rs.tmpl" with { type: "text" };

export function init(args: Args): number {
  const dir = resolve(args.positionals[0] ?? ".");
  const force = boolFlag(args, "force");
  const name = stringFlag(args, "name") ?? humanize(basename(dir));
  const ids = identifiers(name, stringFlag(args, "package"));

  if (existsSync(dir) && readdirSync(dir).length > 0 && !force) {
    console.error(`kiln: ${dir} is not empty (pass --force to write anyway)`);
    return 1;
  }

  const values: Record<string, string> = {
    NAME: ids.display,
    CRATE: ids.crate,
    LIB: ids.lib,
    PACKAGE: ids.package,
    JNI_PACKAGE: ids.jniPackage,
    DOMAIN: ids.domain,
    LEAF: ids.leaf,
    KILN_PATH: kilnDependency(),
    KILN_PATH_INNER: kilnDependency(),
    KILN_ANDROID_PATH_INNER: kilnAndroidDependency(),
  };

  // Every placeholder in one pass, so a template can reference any of them and a
  // missing value shows up as a literal {{X}} in the output rather than silently.
  const fill = (template: string) =>
    template.replace(/\{\{([A-Z_]+)\}\}/g, (whole, key: string) => values[key] ?? whole);

  const write = (path: string, contents: string) => {
    const full = join(dir, path);
    mkdirSync(join(full, ".."), { recursive: true });
    writeFileSync(full, contents);
    console.log(`  created ${path}`);
  };

  console.log(`\nScaffolding ${ids.display} in ${dir}\n`);

  // Shared by desktop and Android: one frontend, one config, one set of commands.
  write("kiln.json", fill(kilnJson));
  write("index.html", fill(indexHtml));
  write("src/main.ts", fill(mainTs));
  write("src/style.css", fill(styleCss));
  write("src/kiln.d.ts", kilnDts);
  write("tsconfig.json", fill(tsconfig));
  write(".gitignore", gitignore);

  // Desktop shell.
  write("native/Cargo.toml", fill(cargoToml));
  write("native/src/main.rs", fill(mainRs));
  write("native/src/assets.rs", fill(assetsRs));

  // Android shell. Same app, different entry point and packaging.
  write("android/Cargo.toml", fill(androidCargo));
  write("android/src/lib.rs", fill(androidLib));
  write("android/src/assets.rs", fill(androidAssets));
  write("android/app/build.gradle", fill(androidBuildGradle));
  write("android/app/settings.gradle", fill(androidSettings));
  write("android/app/gradle.properties", fill(androidGradleProps(androidProps)));
  write("android/app/src/main/AndroidManifest.xml", fill(androidManifest));
  write("android/app/src/main/res/values/themes.xml", fill(androidThemes));
  write(
    `android/app/src/main/java/${ids.package.replace(/\./g, "/")}/MainActivity.java`,
    fill(androidActivity),
  );

  console.log(
    [
      "",
      "Next:",
      `  cd ${args.positionals[0] ?? "."}`,
      "  kiln dev              # desktop, with hot reload",
      "  kiln build --size     # desktop release binary",
      "  kiln android build    # APK",
      "  kiln android run      # install and launch on a device or emulator",
      "",
    ].join("\n"),
  );
  return 0;
}

function humanize(slug: string): string {
  const base = slug === "." || slug === "" ? "kiln-app" : slug;
  return base
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase())
    .trim();
}

/** gradle.properties has no placeholders of its own; kept as a function so the
 *  import is not flagged unused when the file is written verbatim. */
function androidGradleProps(template: string): string {
  return template;
}
