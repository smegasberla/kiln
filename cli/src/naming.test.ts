import { describe, expect, test } from "bun:test";
import { crateName, identifiers, jniPackage, libName } from "./naming.ts";

describe("crateName", () => {
  test("slugifies human names", () => {
    expect(crateName("My Notes")).toBe("my-notes");
    expect(crateName("  Spaces  ")).toBe("spaces");
    expect(crateName("a.b/c")).toBe("a-b-c");
  });

  test("never produces an empty or digit-leading crate", () => {
    expect(crateName("")).toBe("kiln-app");
    expect(crateName("!!!")).toBe("kiln-app");
    expect(crateName("3d viewer")).toBe("app-3d-viewer");
  });
});

describe("libName", () => {
  test("uses underscores", () => {
    expect(libName("my-notes")).toBe("my_notes");
  });

  test("avoids Rust keywords", () => {
    // `mod` is a keyword; a lib named `mod` would not compile.
    expect(libName("mod")).toBe("mod_app");
    expect(libName("type")).toBe("type_app");
  });
});

describe("jniPackage", () => {
  test("mangles dots and underscores the way JNI does", () => {
    expect(jniPackage("com.kiln.app")).toBe("com_kiln_app");
    // Underscores become _1, otherwise the JNI decoder reads them as dots.
    expect(jniPackage("com.kiln.my_notes")).toBe("com_kiln_my_1notes");
  });
});

describe("identifiers", () => {
  test("splits the package for tao and wry", () => {
    const ids = identifiers("My Notes");
    expect(ids.package).toBe("com.kiln.my_notes");
    expect(ids.domain).toBe("com_kiln");
    expect(ids.leaf).toBe("my_notes");
    expect(ids.jniPackage).toBe("com_kiln_my_1notes");
    expect(ids.lib).toBe("my_notes");
  });

  test("honours a package override", () => {
    const ids = identifiers("Notes", "dev.example.notes");
    expect(ids.package).toBe("dev.example.notes");
    expect(ids.domain).toBe("dev_example");
    expect(ids.leaf).toBe("notes");
  });

  test("a single-segment package still yields a domain", () => {
    const ids = identifiers("Notes", "notes");
    expect(ids.domain).toBe("com");
    expect(ids.leaf).toBe("notes");
  });
});
