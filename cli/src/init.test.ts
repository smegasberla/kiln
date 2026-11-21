import { afterEach, describe, expect, spyOn, test } from "bun:test";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Args } from "./args.ts";
import { init } from "./commands/init.ts";

/** `init` prints a file per line; the tests care about the tree, not the log. */
const quiet = <T>(run: () => T): T => {
  const log = spyOn(console, "log").mockImplementation(() => {});
  try {
    return run();
  } finally {
    log.mockRestore();
  }
};

const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function scaffold(flags: Args["flags"] = {}): string {
  const root = mkdtempSync(join(tmpdir(), "voussoir-init-"));
  roots.push(root);
  const dir = join(root, "app");
  const args: Args = { command: "init", positionals: [dir], flags, help: false };
  expect(quiet(() => init(args))).toBe(0);
  return dir;
}

describe("voussoir init", () => {
  test("a default project has both shells", () => {
    const dir = scaffold({ name: "Both Shells" });
    expect(existsSync(join(dir, "native", "src", "main.rs"))).toBe(true);
    expect(existsSync(join(dir, "android", "src", "lib.rs"))).toBe(true);
  });

  test("--mobile writes no desktop crate", () => {
    // The point of the flag: an app that only ships to Play should not carry a
    // `native/` tree it never builds. A stale one is worse than none, because
    // `checkParity` then compares against a command list nobody maintains.
    const dir = scaffold({ name: "Mobile Only", mobile: true });
    expect(existsSync(join(dir, "native"))).toBe(false);
    expect(existsSync(join(dir, "android", "src", "lib.rs"))).toBe(true);
    expect(existsSync(join(dir, "index.html"))).toBe(true);
  });

  test("--mobile still writes every Android file a build needs", () => {
    const dir = scaffold({ name: "Mobile Only", mobile: true });
    const java = join(dir, "android", "app", "src", "main", "java", "com");
    expect(existsSync(join(dir, "android", "app", "build.gradle"))).toBe(true);
    expect(existsSync(join(dir, "android", "app", "src", "main", "AndroidManifest.xml"))).toBe(true);

    // `android build` compiles Java only after wry generates `MainActivity`, and
    // the shell is what the manifest names, so a missing activity is a broken
    // scaffold that still reports success.
    const activity = readdirSync(java, { recursive: true }).some((entry) =>
      String(entry).endsWith("MainActivity.java"),
    );
    expect(activity).toBe(true);
  });

  test("a non-empty directory is refused without --force", () => {
    const dir = scaffold({ name: "Once" });
    const args: Args = { command: "init", positionals: [dir], flags: {}, help: false };
    const error = spyOn(console, "error").mockImplementation(() => {});
    try {
      expect(init(args)).toBe(1);
    } finally {
      error.mockRestore();
    }
  });

  test("--force rewrites over a non-empty directory", () => {
    const dir = scaffold({ name: "Once" });
    const args: Args = { command: "init", positionals: [dir], flags: { force: true }, help: false };
    expect(quiet(() => init(args))).toBe(0);
  });

  test("the scaffolded crate names the app it belongs to", () => {
    const dir = scaffold({ name: "Mobile Only", mobile: true });
    const cargo = readFileSync(join(dir, "android", "Cargo.toml"), "utf8");
    expect(cargo).toContain("mobile_only");
    const manifest = readFileSync(join(dir, "android", "app", "src", "main", "AndroidManifest.xml"), "utf8");
    expect(manifest).not.toContain("{{");
  });
});
