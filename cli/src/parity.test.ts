import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checkParity, diffCommands, effectiveCommands, registeredCommands } from "./parity.ts";

const source = `
  App::new()
    .command("greet", |_a, p| Ok(p))
    .async_command("slow_task", |_a, p| Ok(p))
    .command("open_settings", |_a, _p| Ok(p))
`;

describe("registeredCommands", () => {
  test("finds sync and async registrations", () => {
    expect(registeredCommands(source)).toEqual(["greet", "open_settings", "slow_task"]);
  });

  test("ignores comments mentioning commands", () => {
    const commented = `
      // .command("ghost", ..) is mentioned here but not registered
      .command("real", |_a, _p| Ok(null))
    `;
    expect(registeredCommands(commented)).toEqual(["real"]);
  });

  test("an empty shell registers nothing", () => {
    expect(registeredCommands("App::new()")).toEqual([]);
  });
});

describe("diffCommands", () => {
  test("reports commands missing on one side", () => {
    // This is the real bug that shipped: the shared frontend called slow_task,
    // which only the desktop crate registered.
    const diff = diffCommands(["greet", "slow_task"], ["greet"]);
    expect(diff.onlyDesktop).toEqual(["slow_task"]);
    expect(diff.onlyAndroid).toEqual([]);
  });

  test("an extra command on Android is reported too", () => {
    const diff = diffCommands(["greet"], ["greet", "android_only"]);
    expect(diff.onlyAndroid).toEqual(["android_only"]);
  });

  test("matching sets produce no diff", () => {
    const diff = diffCommands(["a", "b"], ["b", "a"]);
    expect(diff.onlyDesktop).toEqual([]);
    expect(diff.onlyAndroid).toEqual([]);
  });
});

/** A throwaway project tree, so these cases touch no real app. */
function scaffold(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), "kiln-parity-"));
  for (const [path, contents] of Object.entries(files)) {
    const full = join(root, path);
    mkdirSync(join(full, ".."), { recursive: true });
    writeFileSync(full, contents);
  }
  return root;
}

describe("checkParity", () => {
  test("a mobile-only project has nothing to compare", () => {
    // `kiln init --mobile` writes no desktop crate. Reporting a missing
    // `native/src/main.rs` would fail every `kiln android build` in that shape.
    const root = scaffold({
      "android/src/lib.rs": `.command("greet", |_a, p| Ok(p))`,
    });
    const result = checkParity(root);
    expect(result.ok).toBe(true);
    expect(result.message).toBe("mobile only; skipping parity");
  });

  test("a mobile-only project with no android crate is still an error", () => {
    // Neither shell exists: the tree is not a Kiln project at all.
    const root = scaffold({ "kiln.json": "{}" });
    expect(checkParity(root).ok).toBe(false);
  });

  test("two crates that disagree are reported", () => {
    const root = scaffold({
      "native/src/main.rs": `.command("greet", |_a, p| Ok(p))`,
      "android/src/lib.rs": `.command("wave", |_a, p| Ok(p))`,
    });
    const result = checkParity(root);
    expect(result.ok).toBe(false);
    expect(result.message).toContain("greet");
    expect(result.message).toContain("wave");
  });

  test("a desktop-only project passes", () => {
    const root = scaffold({ "native/src/main.rs": `.command("greet", |_a, p| Ok(p))` });
    const result = checkParity(root);
    expect(result.ok).toBe(true);
    expect(result.message).toContain("no android crate");
  });
});

describe("effectiveCommands", () => {
  test("a shared command file is followed instead of reporting nothing", () => {
    // An app that puts one command list in a file both shells include has no
    // list of its own to compare, so reading only the crate root would report
    // zero commands on both sides and pass without checking anything.
    const root = scaffold({
      "shared/commands.rs": `.command("greet", |_a, p| Ok(p))`,
      "native/src/main.rs": `#[path = "../../shared/commands.rs"] mod commands;`,
    });
    expect(effectiveCommands(`#[path = "../../shared/commands.rs"] mod commands;`, join(root, "native/src"))).toEqual([
      "greet",
    ]);
  });

  test("an include! is followed too", () => {
    const root = scaffold({
      "shared/commands.rs": `.command("wave", |_a, p| Ok(p))`,
      "native/src/main.rs": `include!("../../shared/commands.rs");`,
    });
    expect(effectiveCommands(`include!("../../shared/commands.rs");`, join(root, "native/src"))).toEqual([
      "wave",
    ]);
  });

  test("a missing include is ignored rather than fatal", () => {
    // A crate that is mid-edit should report no commands, not crash the build.
    const root = scaffold({ "native/src/main.rs": "fn main() {}" });
    expect(effectiveCommands(`include!("../nope.rs");`, join(root, "native/src"))).toEqual([]);
  });
});

describe("mod declarations", () => {
  test("a plain `mod app;` beside the crate root is followed", () => {
    // The ordinary way to share one command list, and the one that matters most:
    // without it a shared list reports zero commands and passes vacuously.
    const root = scaffold({
      "native/src/app.rs": `.command("share", |_a, p| Ok(p))`,
      "native/src/main.rs": `mod app;\nmod assets;\nfn main() { app::build() }`,
    });
    const source = readFileSync(join(root, "native/src/main.rs"), "utf8");
    expect(effectiveCommands(source, join(root, "native/src"))).toEqual(["share"]);
  });
});
