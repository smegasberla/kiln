/** `voussoir docs`: validate the docs, then build the site that renders them.
 *
 *  There is no mdBook here and no second Markdown parser. `docs/src/*.md` is the
 *  only source, and the website renders those files directly with `Bun.markdown`,
 *  so what a reader sees on GitHub and what the site shows cannot drift apart.
 *
 *  The check half exists because a docs tree rots silently. A chapter missing from
 *  `SUMMARY.md` is unreachable from the sidebar, and a `SUMMARY.md` link to a file
 *  that does not exist renders an empty page rather than an error. Both are cheap
 *  to detect, so `--check` detects them and CI runs it without building anything.
 */

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { run, fail } from "../proc.ts";
import { boolFlag, type Args } from "../args.ts";

/** Where the docs live, resolved from this file rather than the working
 *  directory: `voussoir docs` must work from inside an app project too. */
export function bookDir(): string {
  return resolve(import.meta.dir, "../../../docs");
}

/** Where the website generator lives. */
export function websiteDir(): string {
  return resolve(import.meta.dir, "../../../website");
}

export type Link = { title: string; path: string; line: number };

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
      found.push(...markdownFiles(dir, relative));
    } else if (entry.name.endsWith(".md")) {
      found.push(relative);
    }
  }
  return found.sort();
}

/** Everything wrong with the docs at `src`, as human-readable problems.
 *
 *  Pure apart from reading files, so the tests can build a directory in a temp
 *  path and assert on the result without running the site build.
 */
export function checkBook(src: string): string[] {
  const problems: string[] = [];
  const summaryPath = join(src, "SUMMARY.md");
  if (!existsSync(summaryPath)) {
    return [`${summaryPath} is missing; the site reads its navigation from it`];
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

/** Relative `.md` links that point at a chapter the summary does not list.
 *
 *  The site rewrites `x.md` to `x.html`, so a link to a file outside the summary
 *  becomes a link to a page that was never generated. mdBook failed these at build
 *  time; this is the replacement, because the failure mode is the same.
 */
export function danglingLinks(src: string, links: Link[]): string[] {
  const known = new Set(links.map((link) => link.path));
  const problems: string[] = [];
  for (const file of markdownFiles(src)) {
    if (file === "SUMMARY.md") continue;
    const text = readFileSync(join(src, file), "utf8");
    for (const match of text.matchAll(/\]\(([^)\s]+)\)/g)) {
      const target = match[1]!;
      if (target.startsWith("http") || target.startsWith("#") || target.startsWith("mailto:")) {
        continue;
      }
      const path = target.split("#")[0]!;
      if (!path.endsWith(".md")) continue;
      if (!known.has(path)) {
        problems.push(`${join(src, file)}: links to ${path}, which SUMMARY.md does not list`);
      }
    }
  }
  return problems;
}

/** Validate the docs, then build the website when asked to. */
export async function docs(args: Args): Promise<number> {
  const dir = bookDir();
  const checkOnly = boolFlag(args, "check");

  const src = join(dir, "src");
  const links = parseSummary(readFileSync(join(src, "SUMMARY.md"), "utf8"));
  const problems = [...checkBook(src), ...danglingLinks(src, links)];
  for (const problem of problems) console.error(`voussoir: docs: ${problem}`);
  if (problems.length > 0) return 1;
  console.log(`  docs ok (${links.length} chapters, ${markdownFiles(src).length - 1} files)`);

  if (checkOnly) return 0;

  const site = websiteDir();
  if (!existsSync(join(site, "build.ts"))) {
    fail(`no website/build.ts at ${site}; the CLI and the site have drifted apart`);
  }

  try {
    await run(["bun", "run", "build"], { cwd: site });
  } catch (error) {
    console.error(`voussoir: docs: ${(error as Error).message}`);
    return 1;
  }
  console.log(`  site: ${join(site, "dist", "index.html")}`);
  return 0;
}
