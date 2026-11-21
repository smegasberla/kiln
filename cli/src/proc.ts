/** Tiny process helpers. Bun.spawn only; no execa, no shell. */

export type RunOptions = {
  cwd?: string;
  env?: Record<string, string | undefined>;
  silent?: boolean;
};

/** Run a command to completion. Throws with the captured stderr on failure. */
export async function run(cmd: string[], opts: RunOptions = {}): Promise<string> {
  const proc = Bun.spawn(cmd, {
    cwd: opts.cwd,
    env: { ...process.env, ...opts.env },
    stdout: "pipe",
    stderr: "pipe",
  });

  const [stdout, stderr, code] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);

  if (!opts.silent && stdout.trim()) process.stdout.write(stdout);
  if (code !== 0) {
    throw new Error(`${cmd.join(" ")} exited ${code}\n${stderr.trim()}`);
  }
  return stdout;
}

/** Spawn a long-lived child, inheriting stdio so its output interleaves. */
export function spawn(cmd: string[], opts: RunOptions = {}) {
  return Bun.spawn(cmd, {
    cwd: opts.cwd,
    env: { ...process.env, ...opts.env },
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
}

export function which(bin: string): string | null {
  return Bun.which(bin);
}

export function fail(message: string): never {
  console.error(`voussoir: ${message}`);
  process.exit(1);
}
