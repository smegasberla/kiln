/** `voussoir package`: turn a built app into something a user can install.
 *
 *  Each platform gets the format users there actually expect:
 *
 *  | Platform | Output | Notes |
 *  | --- | --- | --- |
 *  | Linux | `.AppImage` | runs anywhere, no install, no root |
 *  | macOS | `.app` bundle | the on-disk shape Finder and Gatekeeper expect |
 *  | Windows | `.exe` + `.zip` | the binary plus the WebView2 bootstrapper note |
 *
 *  Signing is deliberately not attempted here. It needs credentials the tool
 *  cannot invent, and pretending to sign produces a package that looks trusted
 *  and is not. The command prints exactly what to run instead.
 */

import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,

  rmSync,

  writeFileSync,
} from "node:fs";
import { basename, join, resolve } from "node:path";
import { boolFlag, stringFlag, type Args } from "../args.ts";
import { bundle, cargoBuild, project } from "../pipeline.ts";
import { defaultIcon } from "../icon.ts";
import { writeAssetTable } from "../pipeline.ts";

/** What the packager needs to know about one app. */
export type PackageInput = {
  /** Directory holding the built binary and the assets to ship alongside it. */
  appDir: string;
  binary: string;
  name: string;
  version: string;
  identifier: string;
  icon: string | null;
  /** Files that are not the binary but must be beside it. */
  extraFiles: string[];
};

/** The release version, from the crate's Cargo.toml. */
export function readVersion(crateDir: string): string {
  const manifest = join(crateDir, "Cargo.toml");
  const text = existsSync(manifest) ? readFileSync(manifest, "utf8") : "";
  return /^\s*version\s*=\s*"([^"]+)"/m.exec(text)?.[1] ?? "0.1.0";
}

/** A reverse-DNS identifier derived from the app name, overridable by config. */
export function identifierFor(name: string, overridden?: string): string {
  if (overridden) return overridden;
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `com.voussoir.${slug || "app"}`;
}

/** The `.desktop` entry an AppImage needs to be launchable and installable. */
export function desktopEntry(input: PackageInput, execName: string): string {
  return `[Desktop Entry]
Type=Application
Name=${input.name}
Comment=Created with Voussoir
Exec=${execName}
Icon=${input.identifier}
Terminal=false
Categories=Utility;
X-AppImage-Version=${input.version}
`;
}

/** The `Info.plist` a macOS `.app` needs. */
export function infoPlist(input: PackageInput, execName: string): string {
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>${input.name}</string>
  <key>CFBundleDisplayName</key>
  <string>${input.name}</string>
  <key>CFBundleExecutable</key>
  <string>${execName}</string>
  <key>CFBundleIdentifier</key>
  <string>${input.identifier}</string>
  <key>CFBundleVersion</key>
  <string>${input.version}</string>
  <key>CFBundleShortVersionString</key>
  <string>${input.version}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>LSMinimumSystemVersion</key>
  <string>10.15</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key>
  <true/>
</dict>
</plist>
`;
}

/** Lay out an AppDir the way appimagetool expects. */
export function appDirLayout(input: PackageInput): { [path: string]: string | null } {
  const execName = basename(input.binary);
  const layout: { [path: string]: string | null } = {
    [`${execName}`]: null,
    [`${input.identifier}.desktop`]: desktopEntry(input, execName),
    [`usr/share/applications/${input.identifier}.desktop`]: desktopEntry(input, execName),
  };
  if (input.icon) {
    layout[`${input.identifier}.png`] = null;
    layout[`usr/share/icons/hicolor/256x256/apps/${input.identifier}.png`] = null;
  }
  return layout;
}

export async function runPackage(args: Args): Promise<number> {
  const p = project();
  const release = !boolFlag(args, "debug");
  const crate = join(p.root, "native");

  // A package must contain the embedded frontend, so build first.
  await bundle(p, true);
  writeAssetTable(p);
  const binary = await cargoBuild(p, release);

  const version = readVersion(crate);
  const identifier = identifierFor(
    p.config.name,
    stringFlag(args, "identifier") ?? undefined,
  );
  const icon = stringFlag(args, "icon") ?? null;

  // Without an icon, appimagetool refuses and macOS shows a generic document
  // icon. Draw one instead of failing: a new project has no artwork yet.
  let iconPath: string | null = null;
  if (icon) {
    if (!existsSync(resolve(icon))) {
      console.warn(`voussoir: icon not found at ${icon}; using the generated default`);
    } else {
      iconPath = resolve(icon);
    }
  }
  if (!iconPath) {
    const generated = join(p.root, "dist-package", "icon.png");
    mkdirSync(join(p.root, "dist-package"), { recursive: true });
    await Bun.write(generated, defaultIcon(256));
    iconPath = generated;
  }

  const input: PackageInput = {
    appDir: join(p.root, "dist-package"),
    binary,
    name: p.config.name,
    version,
    identifier,
    icon: iconPath,
    extraFiles: [],
  };

  console.log(`voussoir package: ${input.name} ${version} (${identifier})`);

  const built = await packageForPlatform(input);
  console.log(`\n  ${built}`);
  printSigningSteps(input, built);
  return 0;
}

async function packageForPlatform(input: PackageInput): Promise<string> {
  switch (process.platform) {
    case "darwin":
      return packageMacos(input);
    case "win32":
      return packageWindows(input);
    case "freebsd":
    case "netbsd":
    case "openbsd":
      return packageBsd(input);
    default:
      return packageLinux(input);
  }
}

/** BSD: a staged tree plus a pkg(8) manifest, which is how FreeBSD installs
 *  software. No self-contained single file exists there the way AppImage does. */
async function packageBsd(input: PackageInput): Promise<string> {
  const execName = basename(input.binary);
  const stage = join(input.appDir, "bsd");
  prepare(stage);

  const bindir = join(stage, "usr/local/bin");
  const sharedir = join(stage, "usr/local/share/applications");
  const icondir = join(stage, "usr/local/share/icons/hicolor/256x256/apps");
  mkdirSync(bindir, { recursive: true });
  mkdirSync(sharedir, { recursive: true });

  copyFileSync(input.binary, join(bindir, execName));
  chmodSync(join(bindir, execName), 0o755);
  writeFileSync(join(sharedir, `${input.identifier}.desktop`), desktopEntry(input, execName));

  if (input.icon) {
    mkdirSync(icondir, { recursive: true });
    copyFileSync(input.icon, join(icondir, `${input.identifier}.png`));
  }

  // `+MANIFEST` is what pkg(8) reads; `pkg create -r <stage>` turns the tree into
  // a .pkg. The manifest is written here so the staged tree is inspectable.
  writeFileSync(
    join(stage, "+MANIFEST"),
    [
      `name: ${input.identifier}`,
      `version: "${input.version}"`,
      `origin: local/${input.identifier}`,
      `comment: ${input.name}`,
      "desc: A desktop app built with Voussoir",
      "www: https://github.com/smegasberla/voussoir",
      "maintainer: unset@example.invalid",
      "prefix: /usr/local",
      `abi: "*"`,
      "categories: [x11]",
      "",
    ].join("\n"),
  );

  const pkg = Bun.which("pkg");
  if (!pkg) {
    return stage; // staged tree only; pkg(8) is not on this host
  }
  const out = join(input.appDir, `${input.identifier}-${input.version}.pkg`);
  const proc = Bun.spawn([pkg, "create", "-r", stage, "-o", out], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stderr, code] = await Promise.all([new Response(proc.stderr).text(), proc.exited]);
  if (code !== 0) {
    console.warn(`voussoir: pkg create failed, leaving the staged tree: ${stderr.slice(-200)}`);
    return stage;
  }
  return out;
}

/** AppImage: one self-contained executable file, no install step. */
async function packageLinux(input: PackageInput): Promise<string> {
  const appdir = join(input.appDir, `${basename(input.binary)}.AppDir`);
  prepare(appdir);

  const execName = basename(input.binary);
  copyFileSync(input.binary, join(appdir, execName));
  chmodSync(join(appdir, execName), 0o755);

  const desktop = desktopEntry(input, execName);
  writeFileSync(join(appdir, `${input.identifier}.desktop`), desktop);
  mkdirSync(join(appdir, "usr/share/applications"), { recursive: true });
  writeFileSync(join(appdir, "usr/share/applications", `${input.identifier}.desktop`), desktop);

  if (input.icon) {
    copyFileSync(input.icon, join(appdir, `${input.identifier}.png`));
    mkdirSync(join(appdir, "usr/share/icons/hicolor/256x256/apps"), { recursive: true });
    copyFileSync(
      input.icon,
      join(appdir, "usr/share/icons/hicolor/256x256/apps", `${input.identifier}.png`),
    );
  }

  // AppRun is the entry point; a symlink-free script keeps it portable.
  writeFileSync(
    join(appdir, "AppRun"),
    `#!/bin/sh\nHERE="$(dirname "$(readlink -f "$0")")"\nexec "$HERE/${execName}" "$@"\n`,
  );
  chmodSync(join(appdir, "AppRun"), 0o755);

  const out = join(input.appDir, `${execName}-${input.version}-x86_64.AppImage`);
  const tool = Bun.which("appimagetool");
  if (!tool) {
    // Without appimagetool the AppDir is still a working portable app, so say
    // what was produced rather than failing the whole build.
    console.warn(
      "voussoir: appimagetool not found; leaving a runnable AppDir instead of an AppImage.\n" +
        "      install it from https://github.com/AppImage/appimagetool/releases",
    );
    return appdir;
  }

  const proc = Bun.spawn([tool, appdir, out], {
    env: { ...process.env, ARCH: "x86_64" },
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stderr, code] = await Promise.all([new Response(proc.stderr).text(), proc.exited]);
  if (code !== 0) {
    throw new Error(`appimagetool exited ${code}\n${stderr.slice(-400)}`);
  }
  return out;
}

/** A `.app` bundle, which is what Finder, Gatekeeper, and the Dock expect. */
async function packageMacos(input: PackageInput): Promise<string> {
  const execName = basename(input.binary);
  const app = join(input.appDir, `${input.name}.app`);
  const contents = join(app, "Contents");
  prepare(app);

  mkdirSync(join(contents, "MacOS"), { recursive: true });
  mkdirSync(join(contents, "Resources"), { recursive: true });

  copyFileSync(input.binary, join(contents, "MacOS", execName));
  chmodSync(join(contents, "MacOS", execName), 0o755);
  writeFileSync(join(contents, "Info.plist"), infoPlist(input, execName));

  if (input.icon) {
    copyFileSync(input.icon, join(contents, "Resources", "icon.icns"));
  }
  return app;
}

/** Windows: the .exe, zipped, with a note about WebView2. */
async function packageWindows(input: PackageInput): Promise<string> {
  const out = join(input.appDir, "windows");
  prepare(out);

  const execName = basename(input.binary);
  copyFileSync(input.binary, join(out, execName));
  for (const file of input.extraFiles) {
    copyFileSync(file, join(out, basename(file)));
  }

  writeFileSync(
    join(out, "README.txt"),
    `${input.name} ${input.version}\r\n\r\n` +
      `Requires Microsoft Edge WebView2, which ships with Windows 10 (2020+) and\r\n` +
      `Windows 11. On older systems install the Evergreen WebView2 Runtime:\r\n` +
      `https://developer.microsoft.com/microsoft-edge/webview2/\r\n\r\n` +
      `Run ${execName} to start. This build is unsigned, so Windows may show a\r\n` +
      `SmartScreen warning; see the signing notes from \`voussoir package\`.\r\n`,
  );

  const zip = join(input.appDir, `${basename(execName, ".exe")}-${input.version}-windows-x64.zip`);
  const proc = Bun.spawn(["tar", "-a", "-c", "-f", zip, "-C", out, "."], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stderr, code] = await Promise.all([new Response(proc.stderr).text(), proc.exited]);
  return code === 0 ? zip : out;
}

/** Create (or clear) a directory before writing into it. */
function prepare(dir: string): void {
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
}

/** Print the exact commands for a real, trusted distribution. */
function printSigningSteps(input: PackageInput, artifact: string): void {
  const name = basename(artifact);
  console.log("");
  switch (process.platform) {
    case "darwin":
      console.log(`  Sign:    codesign --deep --force --options runtime \\
    --sign "Developer ID Application: YOUR NAME (TEAMID)" "${artifact}"`);
      console.log(`  Notarize: xcrun notarytool submit "${name}.zip" \\
    --apple-id YOU@EXAMPLE.COM --team-id TEAMID --password APP_PASSWORD --wait`);
      console.log(`  Staple:  xcrun stapler staple "${artifact}"`);
      break;
    case "win32":
      console.log(`  Sign:    signtool sign /fd SHA256 /tr http://timestamp.digicert.com \\
    /td SHA256 /a "${artifact}"`);
      break;
    case "freebsd":
    case "netbsd":
    case "openbsd":
      console.log(`  Install: pkg install -f "${name}" (or copy the staged tree)`);
      console.log(`  Note:    BSD packaging has no signing step; trust comes from the`);
      console.log(`           repository the package is served from.`);
      break;
    default:
      console.log(`  Sign:    gpg --detach-sign --armor "${name}"`);
      console.log(`  Note:    an AppImage runs unsigned. Signing gives users a way to`);
      console.log(`           verify what they downloaded; it does not gate execution.`);
      break;
  }
  console.log("");
  console.log(`  ${input.name} ${input.version}`);
}
