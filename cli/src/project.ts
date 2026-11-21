/** Project discovery and the small `voussoir.json` contract. */

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fail } from "./proc.ts";

export type Project = {
  root: string;
  config: ProjectConfig;
};

export type ProjectConfig = {
  name: string;
  title?: string;
  width?: number;
  height?: number;
  entry: string;
};

const CONFIG_FILE = "voussoir.json";

/** Walk up from `cwd` to the directory holding voussoir.json. */
export function findProject(start = process.cwd()): Project | null {
  let dir = resolve(start);
  for (;;) {
    const configPath = join(dir, CONFIG_FILE);
    if (existsSync(configPath)) {
      return { root: dir, config: readConfig(configPath, dir) };
    }
    const parent = dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

export function requireProject(start = process.cwd()): Project {
  const project = findProject(start);
  if (!project) fail(`no ${CONFIG_FILE} found in ${start} or any parent directory`);
  return project;
}

function readConfig(path: string, root: string): ProjectConfig {
  let raw: unknown;
  try {
    raw = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`${path} is not valid JSON: ${(error as Error).message}`);
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    fail(`${path} must contain a JSON object`);
  }
  const obj = raw as Record<string, unknown>;
  const name = typeof obj.name === "string" ? obj.name : "Voussoir App";
  return {
    name,
    title: typeof obj.title === "string" ? obj.title : undefined,
    width: typeof obj.width === "number" ? obj.width : undefined,
    height: typeof obj.height === "number" ? obj.height : undefined,
    entry: typeof obj.entry === "string" ? obj.entry.replace(/^\//, "") : "index.html",
  };
}

/** The Rust crate directory inside a project. Frontend lives in `src/`,
 *  native code in `native/`. */
export function crateDir(project: Project): string {
  return join(project.root, "native");
}
