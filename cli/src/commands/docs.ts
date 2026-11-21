/** `voussoir docs`: build the documentation book, and check it without mdbook.
 *
 *  The check half exists because a book rots silently. A chapter that is not in
 *  `SUMMARY.md` is unreachable, and a `SUMMARY.md` link to a missing file renders
 *  an empty page rather than an error. Both are cheap to detect, so `voussoir docs
 *  --check` detects them and CI runs it without needing mdbook installed.
 *
 *  The build half is a thin wrapper: mdbook owns rendering, and reimplementing it
 *  would be a second Markdown parser to keep in sync. When mdbook is missing the
 *  command still validates and says exactly how to install it, instead of failing
 *  with a spawn error.
 */

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { run, which, fail } from "../proc.ts";
import { boolFlag, type Args } from "../args.ts";

/** Where the book lives, resolved from this file rather than the working
 *  directory: `voussoir docs` must work from inside an app project too. */
export function bookDir(): string {
  return resolve(import.meta.dir, "../../../docs");
}

/** The mdBook release `voussoir docs` reports as the one to install.
 *
 *  Pinned rather than "latest" so the instructions a user sees are reproducible;
 *  mdBook has not changed its config format across the 0.5 line, so any 0.5.x
 *  works. */
export const MDBOOK_VERSION = "0.5.4";

/** The mdBook release asset for a target triple, or `null` where there is none.
 *
 *  Exported and tested per platform because "make it work on all OSes" is exactly
 *  the claim that rots: a `.zip` extension, an `aarch64` triple and a missing
 *  Windows build are three separate mistakes, and each is one line here.
 */
export function mdbookAsset(triple: string): string | null {
  const os = triple.includes("windows")
    ? "pc-windows-msvc"
    : triple.includes("apple-darwin")
      ? "apple-darwin"
      : triple.includes("linux")
        ? "unknown-linux-gnu"
        : null;
  if (os === null) return null;
  const arch = triple.startsWith("aarch64")
    ? "aarch64"
    : triple.startsWith("x86_64")
      ? "x86_64"
      : null;
  if (arch === null) return null;
  const ext = os === "pc-windows-msvc" ? "zip" : "tar.gz";
  return `mdbook-v${MDBOOK_VERSION}-${arch}-${os}.${ext}`;
}

type Link = { title: string; path: string; line: number };

/** Every chapter reference in a SUMMARY, in order. */
export function parseSummary(text: string): Link[] {
  const links: Link[] = [];
  const lines = text.split(/\r?\n/);
  lines.forEach((line, index) => {
    // `- [Title](path.md)` with arbitrary indent. The title may contain brackets,
    // so the match is anchored on the last `](`.
    const match = /^\s*[-*]\s+\[(.+)\]\(([^)]+)\)\s*$/.exec(line);
    if (!match) return;
    links.push({ title: match[1]!, path: match[2]!, line: index + 1 });
  });
  return links;
}

/** Every `.md` file under `dir`, relative to it, including nested folders. */
export function markdownFiles(dir: string, prefix = ""): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(join(dir, prefix), { withFileTypes: true })) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      // `book/` is build output; a generated page is not a chapter.
      if (relative === "book" || relative === "theme") continue;
      found.push(...markdownFiles(dir, relative));
    } else if (entry.name.endsWith(".md")) {
      found.push(relative);
    }
  }
  return found.sort();
}

/** Everything wrong with the book at `src`, as human-readable problems.
 *
 *  Pure apart from reading files, so the tests can build a directory in a temp
 *  path and assert on the result without running mdbook.
 */
export function checkBook(src: string): string[] {
  const problems: string[] = [];
  const summaryPath = join(src, "SUMMARY.md");
  if (!existsSync(summaryPath)) {
    return [`${summaryPath} is missing; a book needs a SUMMARY.md`];
  }

  const links = parseSummary(readFileSync(summaryPath, "utf8"));
  if (links.length === 0) {
    problems.push(`${summaryPath} lists no chapters`);
  }

  const seen = new Map<string, number>();
  for (const link of links) {
    if (seen.has(link.path)) {
      problems.push(
        `${summaryPath}:${link.line}: ${link.path} is listed twice ` +
          `(first on line ${seen.get(link.path)})`,
      );
    } else {
      seen.set(link.path, link.line);
    }
    if (!existsSync(join(src, link.path))) {
      problems.push(`${summaryPath}:${link.line}: ${link.path} does not exist`);
    }
  }

  // An unlisted chapter is unreachable from the sidebar, which is how a page
  // silently stops existing for readers.
  for (const file of markdownFiles(src)) {
    if (file === "SUMMARY.md") continue;
    if (!seen.has(file)) {
      problems.push(`${join(src, file)} is not listed in SUMMARY.md`);
    }
  }
  return problems;
}

/** The Rust target triple of this machine, as mdBook names it.
 *
 *  Derived from `process.platform`/`arch` rather than shelling out to `rustc`,
 *  so the hint works on a machine without a Rust toolchain: that machine is
 *  exactly the one that cannot `cargo install mdbook` and needs the download.
 */
export function hostTriple(platform = process.platform, arch = process.arch): string {
  const os = platform === "win32" ? "pc-windows-msvc" : platform === "darwin" ? "apple-darwin" : "unknown-linux-gnu";
  const cpu = arch === "arm64" ? "aarch64" : "x86_64";
  return `${cpu}-${os}`;
}

/** Validate the book, then build it when mdbook is available. */
export async function docs(args: Args): Promise<number> {
  const dir = bookDir();
  const checkOnly = boolFlag(args, "check");

  if (!existsSync(join(dir, "book.toml"))) {
    fail(`no book.toml at ${dir}; the CLI and the docs have drifted apart`);
  }

  const problems = checkBook(join(dir, "src"));
  for (const problem of problems) console.error(`voussoir: docs: ${problem}`);
  if (problems.length > 0) return 1;
  console.log(`  book structure ok (${parseSummary(readFileSync(join(dir, "src/SUMMARY.md"), "utf8")).length} chapters)`);

  if (checkOnly) return 0;

  if (!which("mdbook")) {
    // Not a failure: the structure is valid, and installing a renderer is a
    // separate decision. Say how rather than exiting with a spawn error.
    console.error(
      "voussoir: docs: `mdbook` is not on PATH, so the HTML was not rendered.\n" +
        "  Install it with `cargo install mdbook`, or download the release:\n" +
        `  https://github.com/rust-lang/mdBook/releases/tag/v${MDBOOK_VERSION}` +
        (mdbookAsset(hostTriple()) ? `\n  asset: ${mdbookAsset(hostTriple())}` : ""),
    );
    return 1;
  }

  try {
    await run(["mdbook", "build"], { cwd: dir });
  } catch (error) {
    console.error(`voussoir: docs: ${(error as Error).message}`);
    return 1;
  }
  console.log(`  html book: ${join(dir, "book", "index.html")}`);
  return 0;
}
