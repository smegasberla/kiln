/** Minimal argv parsing: `voussoir <command> [flags] [positional]`. */

export type Args = {
  command: string;
  positionals: string[];
  flags: Record<string, string | true>;
  help: boolean;
};

export function parseArgs(argv: string[]): Args {
  const leadingFlags = argv[0]?.startsWith("-") ?? false;
  const [command = "help", ...rest] = leadingFlags ? ["help", ...argv] : argv;
  const positionals: string[] = [];
  const flags: Record<string, string | true> = {};
  let help = false;

  for (let i = 0; i < rest.length; i++) {
    const token = rest[i]!;
    if (token === "-h" || token === "--help") {
      help = true;
    } else if (token.startsWith("--")) {
      const [rawKey, inline] = token.slice(2).split("=", 2);
      const key = rawKey ?? "";
      if (inline !== undefined) {
        flags[key] = inline;
      } else {
        const next = rest[i + 1];
        if (next !== undefined && !next.startsWith("-")) {
          flags[key] = next;
          i++;
        } else {
          flags[key] = true;
        }
      }
    } else {
      positionals.push(token);
    }
  }

  return { command, positionals, flags, help };
}

export function stringFlag(args: Args, name: string): string | undefined {
  const value = args.flags[name];
  return typeof value === "string" ? value : undefined;
}

export function boolFlag(args: Args, name: string): boolean {
  return args.flags[name] === true || args.flags[name] === "true";
}
