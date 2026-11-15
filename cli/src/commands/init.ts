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
import androidDialogs from "../../template/android/KilnDialogs.java.tmpl" with { type: "text" };
import androidMenu from "../../template/android/KilnMenu.java.tmpl" with { type: "text" };
import androidTray from "../../template/android/KilnTray.java.tmpl" with { type: "text" };
import androidHttp from "../../template/android/KilnHttp.java.tmpl" with { type: "text" };
import androidMenuTest from "../../template/android/androidTest/KilnMenuTest.java.tmpl" with { type: "text" };
import androidDialogsTest from "../../template/android/androidTest/KilnDialogsTest.java.tmpl" with { type: "text" };
import androidLifecycleTest from "../../template/android/androidTest/KilnLifecycleTest.java.tmpl" with { type: "text" };
import androidDeviceSupport from "../../template/android/androidTest/KilnDeviceSupport.java.tmpl" with { type: "text" };
import androidDeviceTest from "../../template/android/androidTest/KilnDeviceTest.java.tmpl" with { type: "text" };
import androidCargo from "../../template/android/Cargo.toml.tmpl" with { type: "text" };
import androidLib from "../../template/android/lib.rs.tmpl" with { type: "text" };
import androidAssets from "../../template/android/assets.rs.tmpl" with { type: "text" };

export function init(args: Args): number {
  const dir = resolve(args.positionals[0] ?? ".");
  const force = boolFlag(args, "force");
  // A mobile-only project keeps the frontend and the Android shell and skips the
  // desktop crate entirely: an app that only ever ships to Play has no use for
  // a `native/` tree it will never build, and carrying one invites the two
  // command lists to drift apart.
  const mobile = boolFlag(args, "mobile");
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

  // Desktop shell. A `--mobile` project stops at the Android shell below.
  if (!mobile) {
    write("native/Cargo.toml", fill(cargoToml));
    write("native/src/main.rs", fill(mainRs));
    write("native/src/assets.rs", fill(assetsRs));
  }

  // Android shell. Same app, different entry point and packaging.
  write("android/Cargo.toml", fill(androidCargo));
  write("android/src/lib.rs", fill(androidLib));
  write("android/src/assets.rs", fill(androidAssets));
  write("android/app/build.gradle", fill(androidBuildGradle));
  write("android/app/settings.gradle", fill(androidSettings));
  write("android/app/gradle.properties", fill(androidGradleProps(androidProps)));
  write("android/app/src/main/AndroidManifest.xml", fill(androidManifest));
  write("android/app/src/main/res/values/themes.xml", fill(androidThemes));
  const java = `android/app/src/main/java/${ids.package.replace(/\./g, "/")}`;
  write(`${java}/MainActivity.java`, fill(androidActivity));
  write(`${java}/KilnDialogs.java`, fill(androidDialogs));
  write(`${java}/KilnMenu.java`, fill(androidMenu));
  write(`${java}/KilnTray.java`, fill(androidTray));
  write(`${java}/KilnHttp.java`, fill(androidHttp));

  // Instrumentation tests, in two layers. The first asserts pure data
  // translation, which needs no window: menu ids, intent construction, MIME
  // mapping. The second drives the real round trips on a device, which is the
  // only place a `content://` URI becoming a path, a notification being posted, or
  // a TLS request through the system trust store can be observed at all.
  // `KilnDeviceSupport` is shared plumbing for both, not a test.
  const androidTest = `android/app/src/androidTest/java/${ids.package.replace(/\./g, "/")}`;
  write(`${androidTest}/KilnMenuTest.java`, fill(androidMenuTest));
  write(`${androidTest}/KilnDialogsTest.java`, fill(androidDialogsTest));
  write(`${androidTest}/KilnLifecycleTest.java`, fill(androidLifecycleTest));
  write(`${androidTest}/KilnDeviceSupport.java`, fill(androidDeviceSupport));
  write(`${androidTest}/KilnDeviceTest.java`, fill(androidDeviceTest));

  const next = ["", "Next:", `  cd ${args.positionals[0] ?? "."}`];
  if (!mobile) {
    next.push("  kiln dev              # desktop, with hot reload");
    next.push("  kiln build --size     # desktop release binary");
  }
  next.push("  kiln android build    # APK");
  next.push("  kiln android build --aab  # Play-ready bundle");
  next.push("  kiln android run      # install and launch on a device or emulator");
  next.push("");
  console.log(next.join("\n"));
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
