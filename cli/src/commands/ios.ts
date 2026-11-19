/** `kiln ios <build|run|verify>`: the iOS half of a Kiln project.
 *
 *  The toolchain is entirely different from Android's: a Rust staticlib that
 *  Xcode links, instead of a `.so` an activity loads. Everything else is shared
 *  with the other shells, so the work here is bundling, cross-compiling,
 *  staging the library, generating the Xcode project from its committed
 *  `project.yml`, and driving `xcodebuild` and `simctl`.
 *
 *  Device builds and App Store signing are out of scope on purpose: a device
 *  build compiles, but installing it needs a signed provisioning profile, and
 *  that belongs to whoever owns the Apple account, not to a scaffold.
 */

import { existsSync, mkdirSync, copyFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { boolFlag, stringFlag, type Args } from "../args.ts";
import { bundle, writeAssetTable } from "../pipeline.ts";
import { requireProject, type Project } from "../project.ts";
import { checkParity } from "../parity.ts";
import { run } from "../proc.ts";

/** Targets Kiln builds, and the SDK `xcodebuild` uses for each. */
const TARGETS: Record<string, { target: string; sdk: string; products: string }> = {
  sim: { target: "aarch64-apple-ios-sim", sdk: "iphonesimulator", products: "Debug-iphonesimulator" },
  device: { target: "aarch64-apple-ios", sdk: "iphoneos", products: "Debug-iphoneos" },
};

export function iosDir(project: Project): string {
  return join(project.root, "ios");
}

/** Read `[lib] name` from the iOS crate so the `.a` name is never guessed. */
export function iosLibName(project: Project): string {
  const manifest = join(iosDir(project), "Cargo.toml");
  if (!existsSync(manifest)) {
    throw new Error(`no iOS crate at ${manifest}; run kiln init in this project`);
  }
  const text = require("node:fs").readFileSync(manifest, "utf8") as string;
  const match = /\[lib\][\s\S]*?name\s*=\s*"([^"]+)"/.exec(text);
  if (!match) {
    throw new Error(`${manifest} has no [lib] name; the app has nothing to link`);
  }
  return match[1]!;
}

/** The XcodeGen target/scheme name, read from the committed project.yml.
 *
 *  `kiln init` writes `name: <crate>-iOS` once at the top and uses it for both
 *  the target and the scheme, so the first `name:` line is the single source. */
export function readScheme(project: Project): string {
  const file = join(iosDir(project), "project.yml");
  const text = existsSync(file) ? require("node:fs").readFileSync(file, "utf8") : "";
  const match = /^name:\s*(\S+)/m.exec(text as string);
  if (!match) throw new Error(`${file} has no name; cannot derive the scheme`);
  return match[1]!;
}

/** The bundle identifier, read from the generated project.yml. */
function readBundle(project: Project): string {
  const file = join(iosDir(project), "project.yml");
  const text = existsSync(file) ? require("node:fs").readFileSync(file, "utf8") : "";
  const match = /PRODUCT_BUNDLE_IDENTIFIER:\s*(\S+)/.exec(text as string);
  if (!match) throw new Error(`${file} has no PRODUCT_BUNDLE_IDENTIFIER`);
  return match[1]!;
}

export async function build(args: Args): Promise<number> {
  const project = requireProject();
  const variant = stringFlag(args, "target") === "device" ? "device" : "sim";
  const spec = TARGETS[variant]!;
  const lib = iosLibName(project);
  const ios = iosDir(project);

  // A command registered on only one shell fails at runtime on the other, with
  // "unknown command". Catch it at build time instead.
  const parity = checkParity(project.root);
  if (!parity.ok) {
    console.error(`kiln: command parity problem between the shells\n  ${parity.message}`);
    return 1;
  }
  console.log(`  ${parity.message}`);

  // The frontend must be bundled before it can be embedded.
  console.log(`kiln ios build: ${project.config.name} (${variant})`);
  await bundle(project, true);
  writeAssetTable(project, ios);

  const env = { ...process.env };

  await run(["cargo", "build", "--release", "--target", spec.target], {
    cwd: ios,
    env,
  });

  const archive = join(ios, "target", spec.target, "release", `lib${lib}.a`);
  if (!statSync(archive, { throwIfNoEntry: false })) {
    throw new Error(`cargo finished but ${archive} is missing`);
  }
  const libDir = join(ios, "lib");
  mkdirSync(libDir, { recursive: true });
  copyFileSync(archive, join(libDir, `lib${lib}.a`));
  console.log(`  staged lib/lib${lib}.a`);

  // XcodeGen turns the committed project.yml into the .xcodeproj. The project
  // file itself is generated output (gitignored), so a stale one can never
  // disagree with the source of truth.
  if (Bun.which("xcodegen")) {
    await run(["xcodegen", "generate"], { cwd: ios, silent: true });
  } else if (!existsSync(join(ios, `${readScheme(project)}.xcodeproj`))) {
    throw new Error(
      "no xcodegen found and no generated .xcodeproj either. Install one with\n" +
        "  brew install xcodegen",
    );
  }

  await run(
    [
      "xcodebuild",
      "-project",
      join(ios, `${readScheme(project)}.xcodeproj`),
      "-scheme",
      readScheme(project),
      "-sdk",
      spec.sdk,
      "-configuration",
      "Debug",
      "-derivedDataPath",
      join(ios, "build"),
      "CODE_SIGNING_ALLOWED=NO",
      "CODE_SIGN_IDENTITY=",
      "build",
    ],
    { cwd: ios },
  );

  const app = findApp(project, variant);
  if (!app) {
    throw new Error("xcodebuild finished but no .app was produced");
  }
  console.log(`  ${app} (${(statSync(app).size / 1048576).toFixed(1)} MB)`);
  return 0;
}

/** Install and launch on a simulator, then tail the app's stdout.
 *
 *  `--console-pty` is the simulator's logcat: the Rust staticlib's stdout and
 *  the shell's own diagnostics arrive on the terminal it is attached to. */
export async function runOnDevice(args: Args): Promise<number> {
  const project = requireProject();
  const bundle = readBundle(project);
  const udid = await bootedSimulator();
  const app = findApp(project, "sim");
  if (!app) {
    console.error("kiln: no simulator app yet; run `kiln ios build` first");
    return 1;
  }

  console.log(`kiln ios run: ${bundle}`);
  await run(["xcrun", "simctl", "install", udid, app], { silent: true });
  await run(["xcrun", "simctl", "terminate", udid, bundle], { silent: true });

  console.log("  launched. output:\n");
  const proc = Bun.spawn(
    ["xcrun", "simctl", "launch", "--console-pty", udid, bundle],
    { stdout: "inherit", stderr: "inherit" },
  );
  process.on("SIGINT", () => {
    proc.kill();
    process.exit(0);
  });
  await proc.exited;
  return 0;
}

/** Build (unless told not to), install, launch, and assert the app stayed up.
 *
 *  The console marker from the shell's entry proves the staticlib loaded and
 *  Rust ran; six seconds without a crash then covers window creation. The page
 *  round-trip on a simulator is Phase I-4's job, where CI can assert it the way
 *  the Android emulator job does. */
export async function verify(args: Args): Promise<number> {
  const project = requireProject();
  const bundle = readBundle(project);
  const udid = await bootedSimulator();

  if (!boolFlag(args, "no-build")) {
    const code = await build({ ...args, ["no-build" as never]: undefined } as Args);
    if (code !== 0) return code;
  }
  const app = findApp(project, "sim");
  if (!app) {
    console.error("kiln: no simulator app; run `kiln ios build` first");
    return 1;
  }

  await run(["xcrun", "simctl", "install", udid, app], { silent: true });
  await run(["xcrun", "simctl", "terminate", udid, bundle], { silent: true });

  const proc = Bun.spawn(["xcrun", "simctl", "launch", "--console-pty", udid, bundle], {
    stdout: "pipe",
  });
  const timer = setTimeout(() => proc.kill(), 20000);
  const text = await new Response(proc.stdout).text();
  clearTimeout(timer);

  if (!text.includes("KILN_IOS_START")) {
    console.error("kiln ios verify: the app never reached its Rust entry");
    return 1;
  }
  console.log(`ok   ios: Rust entry reached, app running (${bundle})`);
  return 0;
}

/** The UDID of a booted simulator, booting the first available one otherwise. */
async function bootedSimulator(): Promise<string> {
  const listed = Bun.spawnSync(["xcrun", "simctl", "list", "devices"], {
    stdout: "pipe",
  });
  const text = listed.stdout.toString();
  for (const line of text.split("\n")) {
    if (line.includes("(Booted)")) {
      const match = /\(([0-9A-Fa-f-]+)\)/.exec(line);
      if (match) return match[1]!;
    }
  }
  const available = Bun.spawnSync(["xcrun", "simctl", "list", "devices", "available"], {
    stdout: "pipe",
  });
  for (const line of available.stdout.toString().split("\n")) {
    if (!line.includes("com.apple.CoreSimulator.SimDeviceType")) {
      const match = /\(([0-9A-Fa-f-]{36})\)/.exec(line);
      if (match) {
        await run(["xcrun", "simctl", "boot", match[1]!], { silent: true });
        return match[1]!;
      }
    }
  }
  throw new Error(
    "no iOS simulator found. Install Xcode and create one with\n" +
      "  xcrun simctl create 'iPhone 16' 'iPhone 16'",
  );
}

/** The built .app for a variant, or null when it has not been built. */
export function findApp(project: Project, variant: keyof typeof TARGETS): string | null {
  const spec = TARGETS[variant];
  if (!spec) return null;
  const base = join(iosDir(project), "build", "Build", "Products", spec.products);
  if (!existsSync(base)) return null;
  let best: string | null = null;
  let bestTime = 0;
  for (const entry of require("node:fs").readdirSync(base, { withFileTypes: true })) {
    const full = join(base, entry.name);
    if (entry.isDirectory() && entry.name.endsWith(".app")) {
      const time = statSync(full).mtimeMs;
      if (!best || time > bestTime) {
        best = full;
        bestTime = time;
      }
    }
  }
  return best;
}
