import { describe, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { checkBook, danglingLinks, markdownFiles, parseSummary } from "./docs.ts";

/** A book on disk, so the checks run against real files rather than mocks. */
function book(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), "voussoir-docs-"));
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
  test("finds nested chapters and ignores non-markdown", () => {
    const root = book({
      "a.md": "# a",
      "sub/b.md": "# b",
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

describe("danglingLinks", () => {
  test("accepts a chapter link the summary lists", () => {
    const root = book({
      "SUMMARY.md": "- [Intro](intro.md)\n- [Next](next.md)\n",
      "intro.md": "# Intro\n\nSee [next](next.md).",
      "next.md": "# Next",
    });
    expect(danglingLinks(root, parseSummary("- [Intro](intro.md)\n- [Next](next.md)\n"))).toEqual([]);
  });

  test("reports a relative chapter link the summary does not list", () => {
    // The site rewrites `x.md` to `x.html`; an unlisted file has no page, so the
    // link would 404. This is the failure mdBook used to catch at build time.
    const root = book({
      "SUMMARY.md": "- [Intro](intro.md)\n",
      "intro.md": "# Intro\n\nSee [gone](gone.md).",
      "gone.md": "# Gone",
    });
    const problems = danglingLinks(root, parseSummary("- [Intro](intro.md)\n"));
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("gone.md");
  });

  test("ignores external links, fragments and non-markdown targets", () => {
    const root = book({
      "SUMMARY.md": "- [Intro](intro.md)\n",
      "intro.md": [
        "# Intro",
        "[site](https://example.com/x.md)",
        "[jump](#section)",
        "[mail](mailto:a@b.c)",
        "[asset](../brand/voussoir.png)",
      ].join("\n"),
    });
    expect(danglingLinks(root, parseSummary("- [Intro](intro.md)\n"))).toEqual([]);
  });
});
