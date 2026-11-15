import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

/** Where the templates live, relative to this file. */
const androidDir = join(import.meta.dir, "..", "template", "android");

const read = (name: string) => readFileSync(join(androidDir, name), "utf8");

/** Every `static native` the activity declares, with its Java return type. */
export function javaNatives(source: string): Map<string, string> {
  const found = new Map<string, string>();
  const re = /static native\s+([\w.$<>\[\]]+)\s+(\w+)\s*\(/g;
  for (const match of source.matchAll(re)) found.set(match[2]!, match[1]!);
  return found;
}

/**
 * The `Java_<package>_MainActivity_<name>` exports in the app's Rust crate.
 *
 * The return type sits after the closing paren of a multi-line signature, so it
 * is read from the text between the name and the opening brace of the body.
 */
export function rustExports(source: string): Map<string, string> {
  const found = new Map<string, string>();
  const re = /pub extern "C" fn Java_\{\{JNI_PACKAGE\}\}_MainActivity_(\w+)\s*\(/g;
  for (const match of source.matchAll(re)) {
    const rest = source.slice(match.index! + match[0].length);
    const body = rest.indexOf('{');
    const signature = body === -1 ? rest : rest.slice(0, body);
    const ret = /->\s*([\w:<>]+)\s*$/.exec(signature.trimEnd());
    found.set(match[1]!, ret ? ret[1]! : "void");
  }
  return found;
}

/**
 * The exports the activity itself declares.
 *
 * `Java_..._android_main` is left out: nothing in the app declares it, because
 * `register_jni!` expands to the activity-side hook that calls it.
 */
export function declaredExports(source: string): Map<string, string> {
  return new Map([...rustExports(source)].filter(([name]) => name.startsWith("kiln")));
}

describe("Java -> Rust entry points", () => {
  test("the activity and the app crate export the same set", () => {
    // A name on only one side is an UnsatisfiedLinkError at launch, not a build
    // error, so nothing catches it without this.
    const java = javaNatives(read("MainActivity.java.tmpl"));
    const rust = declaredExports(read("lib.rs.tmpl"));
    expect([...java.keys()].sort()).toEqual([...rust.keys()].sort());
  });

  test("the five shell callbacks are the ones exported", () => {
    const rust = declaredExports(read("lib.rs.tmpl"));
    expect([...rust.keys()].sort()).toEqual([
      "kilnCompleteDialog",
      "kilnFailDialog",
      "kilnLifecycle",
      "kilnMenuClicked",
      "kilnTrayClicked",
    ]);
  });

  test("kilnTrayClicked reports whether the click landed", () => {
    const rust = declaredExports(read("lib.rs.tmpl"));
    expect(rust.get("kilnTrayClicked")).toBe("jni::sys::jboolean");

    const java = javaNatives(read("MainActivity.java.tmpl"));
    expect(java.get("kilnTrayClicked")).toBe("boolean");
  });

  test("a boolean return on either side has one on the other", () => {
    // JNI matches the symbol by name, not by type: a `void` export called from a
    // `boolean` declaration reads whatever happens to be in the return register.
    const java = javaNatives(read("MainActivity.java.tmpl"));
    const rust = declaredExports(read("lib.rs.tmpl"));

    const javaBoolean = (type: string | undefined) => type === "boolean";
    const rustBoolean = (type: string | undefined) => type === "jni::sys::jboolean";

    for (const [name, type] of java) {
      expect([name, rustBoolean(rust.get(name))]).toEqual([name, javaBoolean(type)]);
    }
  });
});

describe("Rust -> Java entry points", () => {
  /** Every `call_method` target the shell names, i.e. the methods it expects to find. */
  const calledMethods = (source: string) => {
    const found = new Set<string>();
    const re = /call_method\(\s*(?:activity|&?[\w.]+)\s*,\s*"(\w+)"/g;
    for (const match of source.matchAll(re)) found.add(match[1]!);
    return found;
  };

  test("every method the shell calls is declared by the activity", () => {
    // The other direction from the JNI exports: a renamed activity method is a
    // NoSuchMethodError at the moment the shell calls it, which on Android means
    // a silently dead picker or menu rather than a failed build.
    const platform = readFileSync(join(import.meta.dir, "..", "..", "kiln", "src", "platform.rs"), "utf8");
    const declared = new Set(
      [...read("MainActivity.java.tmpl").matchAll(/public\s+[\w.$\[\]]+\s+(kiln\w+)\s*\(/g)].map(
        (match) => match[1]!,
      ),
    );

    const called = [...calledMethods(platform)].filter((name) => name.startsWith("kiln"));
    expect(called.length).toBeGreaterThan(0);
    for (const name of called) expect([name, declared.has(name)]).toEqual([name, true]);
  });
});

describe("regeneration", () => {
  const fill = (template: string) =>
    template.replace(/\{\{PACKAGE\}\}/g, "com.kiln.counter").replace(/\{\{LIB\}\}/g, "counter");

  /** Every Java file the template ships, as [template path, example path]. */
  const javaFiles: [string, string][] = [
    ["MainActivity.java.tmpl", "main/java/com/kiln/counter/MainActivity.java"],
    ["KilnDialogs.java.tmpl", "main/java/com/kiln/counter/KilnDialogs.java"],
    ["KilnMenu.java.tmpl", "main/java/com/kiln/counter/KilnMenu.java"],
    ["KilnTray.java.tmpl", "main/java/com/kiln/counter/KilnTray.java"],
    ["KilnHttp.java.tmpl", "main/java/com/kiln/counter/KilnHttp.java"],
    ["androidTest/KilnMenuTest.java.tmpl", "androidTest/java/com/kiln/counter/KilnMenuTest.java"],
    [
      "androidTest/KilnDialogsTest.java.tmpl",
      "androidTest/java/com/kiln/counter/KilnDialogsTest.java",
    ],
    [
      "androidTest/KilnLifecycleTest.java.tmpl",
      "androidTest/java/com/kiln/counter/KilnLifecycleTest.java",
    ],
    [
      "androidTest/KilnDeviceSupport.java.tmpl",
      "androidTest/java/com/kiln/counter/KilnDeviceSupport.java",
    ],
    [
      "androidTest/KilnDeviceTest.java.tmpl",
      "androidTest/java/com/kiln/counter/KilnDeviceTest.java",
    ],
  ];

  for (const [template, example] of javaFiles) {
    test(`the example ${example.split("/").pop()} is the template filled`, () => {
      const path = join(
        import.meta.dir,
        "..",
        "..",
        "kiln-android",
        "example",
        "android",
        "app",
        "src",
        example,
      );
      expect(readFileSync(path, "utf8")).toBe(fill(read(template)));
    });
  }

  test("the example crate keeps the same exports with the package spelled out", () => {
    const example = readFileSync(
      join(import.meta.dir, "..", "..", "kiln-android", "example", "android", "src", "lib.rs"),
      "utf8",
    ).replace(/Java_com_kiln_counter_MainActivity_/g, "Java_{{JNI_PACKAGE}}_MainActivity_");

    expect([...declaredExports(example).keys()].sort()).toEqual(
      [...declaredExports(read("lib.rs.tmpl")).keys()].sort(),
    );
  });
});
