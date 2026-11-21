import { describe, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { checkBook, hostTriple, markdownFiles, mdbookAsset, parseSummary } from "./docs.ts";

/** A book on disk, so the checks run against real files rather than mocks. */
function book(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), "kiln-docs-"));
  for (const [path, content] of Object.entries(files)) {
    const full = join(root, path);
    mkdirSync(join(full, ".."), { recursive: true });
    writeFileSync(full, content);
  }
  return root;
}

describe("parseSummary", () => {
  test("collects titled links in order", () => {
    const links = parseSummary("# Summary\n\n- [Intro](intro.md)\n- [Deep](sub/deep.md)\n");
    expect(links).toEqual([
      { title: "Intro", path: "intro.md", line: 3 },
      { title: "Deep", path: "sub/deep.md", line: 4 },
    ]);
  });

  test("ignores prose, headings and links inside sentences", () => {
    const links = parseSummary(
      "# Summary\n\nRead [Intro](intro.md) first.\n\n## Part\n\n- [Real](real.md)\n",
    );
    expect(links).toHaveLength(1);
    expect(links[0]?.path).toBe("real.md");
  });

  test("handles nested indentation and handles brackets in titles", () => {
    const links = parseSummary("  - [A [b] c](nested.md)\n");
    expect(links[0]?.title).toBe("A [b] c");
    expect(links[0]?.path).toBe("nested.md");
  });
});

describe("markdownFiles", () => {
  test("finds nested chapters and skips build output", () => {
    const root = book({
      "a.md": "# a",
      "sub/b.md": "# b",
      "book/generated.md": "# no",
      "theme/x.md": "# no",
      "note.txt": "no",
    });
    expect(markdownFiles(root)).toEqual(["a.md", "sub/b.md"]);
  });
});

describe("checkBook", () => {
  test("accepts a book whose summary matches its files", () => {
    const root = book({
      "SUMMARY.md": "- [Intro](intro.md)\n",
      "intro.md": "# Intro",
    });
    expect(checkBook(root)).toEqual([]);
  });

  test("reports a missing chapter file", () => {
    const root = book({ "SUMMARY.md": "- [Gone](gone.md)\n" });
    const problems = checkBook(root);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("gone.md does not exist");
  });

  test("reports an orphan chapter", () => {
    const root = book({
      "SUMMARY.md": "- [Intro](intro.md)\n",
      "intro.md": "# Intro",
      "orphan.md": "# Orphan",
    });
    const problems = checkBook(root);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("orphan.md is not listed");
  });

  test("reports a duplicate entry with both line numbers", () => {
    const root = book({
      "SUMMARY.md": "- [Intro](intro.md)\n- [Again](intro.md)\n",
      "intro.md": "# Intro",
    });
    const problems = checkBook(root);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("listed twice");
    expect(problems[0]).toContain("line 1");
  });

  test("reports a summary with no chapters", () => {
    const root = book({ "SUMMARY.md": "# Summary\n\nNothing yet.\n" });
    const problems = checkBook(root);
    expect(problems[0]).toContain("lists no chapters");
  });

  test("reports a missing summary rather than throwing", () => {
    const root = book({ "intro.md": "# Intro" });
    const problems = checkBook(root);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("SUMMARY.md is missing");
  });

  test("finds every problem at once, not just the first", () => {
    const root = book({
      "SUMMARY.md": "- [Intro](intro.md)\n- [Gone](gone.md)\n",
      "intro.md": "# Intro",
      "orphan.md": "# Orphan",
    });
    const problems = checkBook(root);
    expect(problems).toHaveLength(2);
    expect(problems.join("\n")).toContain("gone.md");
    expect(problems.join("\n")).toContain("orphan.md");
  });
});

describe("hostTriple", () => {
  test("names the three desktop targets the way mdBook does", () => {
    expect(hostTriple("linux", "x64")).toBe("x86_64-unknown-linux-gnu");
    expect(hostTriple("win32", "x64")).toBe("x86_64-pc-windows-msvc");
    expect(hostTriple("darwin", "arm64")).toBe("aarch64-apple-darwin");
  });

  test("maps every architecture it claims to support", () => {
    expect(hostTriple("darwin", "x64")).toBe("x86_64-apple-darwin");
    expect(hostTriple("linux", "arm64")).toBe("aarch64-unknown-linux-gnu");
  });
});

describe("mdbookAsset", () => {
  test("uses tar.gz on Unix and zip on Windows", () => {
    expect(mdbookAsset("x86_64-unknown-linux-gnu")).toBe("mdbook-v0.5.4-x86_64-unknown-linux-gnu.tar.gz");
    expect(mdbookAsset("x86_64-pc-windows-msvc")).toBe("mdbook-v0.5.4-x86_64-pc-windows-msvc.zip");
    expect(mdbookAsset("aarch64-apple-darwin")).toBe("mdbook-v0.5.4-aarch64-apple-darwin.tar.gz");
    expect(mdbookAsset("x86_64-apple-darwin")).toBe("mdbook-v0.5.4-x86_64-apple-darwin.tar.gz");
  });

  test("returns null where mdBook publishes no asset", () => {
    expect(mdbookAsset("x86_64-unknown-freebsd")).toBeNull();
    expect(mdbookAsset("i686-unknown-linux-gnu")).toBeNull();
  });

  test("every platform this project targets has an asset", () => {
    // The hint exists for the machine that cannot `cargo install`; if it printed
    // nothing there, it would be worse than the plain releases URL.
    for (const [platform, arch] of [
      ["linux", "x64"],
      ["linux", "arm64"],
      ["darwin", "x64"],
      ["darwin", "arm64"],
      ["win32", "x64"],
    ] as const) {
      expect(mdbookAsset(hostTriple(platform, arch))).not.toBeNull();
    }
  });
});
