# How a project is laid out

A Voussoir project is two things in one directory: a frontend Bun can bundle, and a
Rust crate that embeds the result. The split is deliberate, because the two sides
have different failure modes and different reload speeds.

```
myapp/
  voussoir.json                configuration, embedded into the binary
  index.html               entry page; Bun bundles it and everything it imports
  src/                     frontend: TypeScript, JSX, CSS. Bun handles all of it
  native/
    Cargo.toml             shell crate, depends on `voussoir`
    src/main.rs            App builder: window, shell-service commands, run
    src/assets.rs          generated asset table; never edit by hand
    commands/
      Cargo.toml           cdylib in dev, static lib in release
      src/lib.rs           your command table, hot-reloaded by `voussoir dev`
  android/                 Gradle project, scaffolded by `init`
  ios/                     Xcode project, scaffolded by `init`
```

## `voussoir.json`

```json
{
  "name": "Notes",
  "title": "Notes",
  "width": 900,
  "height": 680,
  "entry": "index.html",
  "commands": ["greet", "save"],
  "windows": { "untrusted": { "commands": ["greet"] } }
}
```

Every key is optional. The file is `include_str!`-ed into the binary, so a shipped
app never reads configuration from disk, and a writable working directory cannot
redirect it.

- `name` is the app identity: data directory, bundle name, Android package stem.
- `entry` is the page the first window loads. A `/` prefix is stripped.
- `commands` is the global allowlist. Absent means "everything registered".
- `windows` narrows one label's surface; see [Per-window allowlists](allowlists.md).
- `command_timeout_ms` is how long the page waits for a command before rejecting.
  Defaults to 30000; `0` disables the deadline for commands that legitimately run
  for minutes. A page can override it per call with
  `voussoir.invoke("slow", payload, { timeout: 300000 })`, and read the default as
  `voussoir.timeout`.

## `native/src/assets.rs`

`voussoir build` and `voussoir dev` both write this file. It is a table of `(path, bytes)`
pairs compiled into the binary, which is why the release build has no frontend
directory to lose and no filesystem read on startup. `cargo check` works in a
fresh checkout because `voussoir init` writes an empty table first.

## Why the command crate is separate

`native/commands` is a `cdylib` in development and a static library in release.
Keeping it in its own crate is what makes the hot-reload loop cheap: `voussoir dev`
rebuilds *only* that crate and swaps the shared object into the running shell,
without recompiling the shell or restarting the process.

The cost is one rule that matters: statics inside that crate do not survive a
reload, because the library that owned them is unloaded. State that must survive
belongs in `voussoir::state`, which the shell owns. See
[Hot reload](hot-reload.md).
