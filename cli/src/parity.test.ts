import { describe, expect, test } from "bun:test";
import { diffCommands, registeredCommands } from "./parity.ts";

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
