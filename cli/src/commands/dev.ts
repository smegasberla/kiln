/** `kiln dev`: Bun dev server with HMR + the Rust shell pointed at it.
 *
 *  The dev server runs in this process, not as a child. That matters twice:
 *  `kiln` is distributed as a compiled binary with no `.ts` files on disk, so
 *  spawning `bun dev-server.ts` would break the installed CLI; and an
 *  in-process server means one Ctrl-C stops everything.
 *
 *  HMR comes from importing the project's entry HTML directly. Bun serves the
 *  transformed HTML and watches everything it pulls in, so edits to TS/CSS
 *  reload in the page without touching Rust. `bun --hot` on the CLI process is
 *  what makes the import re-evaluate after an edit.
 */

import { join } from "node:path";
import { spawn } from "../proc.ts";
import { writeAssetTable } from "../pipeline.ts";
import { crateDir, requireProject } from "../project.ts";
import { stringFlag, type Args } from "../args.ts";

export async function dev(args: Args): Promise<number> {
  const project = requireProject();
  const port = Number(stringFlag(args, "port") ?? 5177);
  const crate = crateDir(project);

  if (!Bun.which("cargo")) {
    console.error("kiln: `cargo` is not on PATH; install Rust first");
    return 1;
  }

  // The shell embeds a table so `cargo check` works while editing Rust; in dev
  // the webview never reads it.
  writeAssetTable(project);

  const server = await startServer(project.root, project.config.entry, port);

  const shell = spawn(["cargo", "run"], {
    cwd: crate,
    env: { ...process.env, KILN_DEV_URL: `http://127.0.0.1:${port}` },
  });

  const shutdown = () => {
    server.stop(true);
    shell.kill();
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);

  const code = await shell.exited;
  server.stop(true);
  return code;
}

/** Import the entry HTML and serve it. Bun's `development: true` gives the
 *  transform pipeline plus the HMR client; it is zero-config by design. */
async function startServer(root: string, entry: string, port: number) {
  let page: unknown;
  try {
    page = (await import(join(root, entry))).default;
  } catch (error) {
    console.error(`kiln: could not load ${entry}: ${(error as Error).message}`);
    process.exit(1);
  }
  if (page === undefined) {
    console.error(`kiln: ${entry} has no default export after bundling`);
    process.exit(1);
  }

  const server = Bun.serve({
    port,
    development: true,
    routes: {
      "/": page as never,
      [`/${entry}`]: page as never,
    },
    fetch(req) {
      return new Response(`not found: ${new URL(req.url).pathname}`, { status: 404 });
    },
  });

  console.log(`  dev server http://127.0.0.1:${server.port}/`);
  return server;
}
