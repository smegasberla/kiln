import { describe, expect, test } from "bun:test";
import { devEnv } from "./dev.ts";

describe("devEnv", () => {
  const base = { PATH: "/usr/bin", KILN_VERBOSE: "1" };

  test("points the shell at the dev server", () => {
    expect(devEnv(5177, false, base).KILN_DEV_URL).toBe("http://127.0.0.1:5177");
    expect(devEnv(6000, false, base).KILN_DEV_URL).toBe("http://127.0.0.1:6000");
  });

  test("sets the verbose flag explicitly when on", () => {
    expect(devEnv(5177, true, base).KILN_VERBOSE).toBe("1");
  });

  test("sets it explicitly to off, overriding an inherited export", () => {
    // A shell profile that exports KILN_VERBOSE=1 would otherwise make every
    // `kiln dev` verbose, including a teammate's.
    expect(devEnv(5177, false, base).KILN_VERBOSE).toBe("0");
  });

  test("keeps the rest of the environment", () => {
    expect(devEnv(5177, false, base).PATH).toBe("/usr/bin");
  });
});
