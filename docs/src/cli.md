# Command-line reference

Every command and flag the `voussoir` CLI accepts, what each one does, and what it
does not do. Run `voussoir help` to see the same list from the binary, which is the
version that matches your install.

## Theory: why the CLI exists at all

A Voussoir app is two build systems that have to agree: Bun bundles the frontend,
Cargo compiles the shell. Neither knows about the other, and the point where they
meet (the asset table, the dev URL, the hot library) is where a hand-rolled setup
always drifts.

The CLI is that meeting point and nothing more. It is deliberately small: eleven
subcommands across nine commands, each a thin wrapper over a tool you could call
yourself. That is a design
constraint worth knowing, because it tells you what to do when it lacks a flag you
want: call the underlying tool directly, and tell the CLI about the result through
`voussoir.json` or an environment variable.

It also runs on Bun, and **Bun is development-time only**. Nothing the CLI produces
contains a JavaScript runtime.

## The commands

| Command | What it does | Needs |
| --- | --- | --- |
| `init [dir]` | scaffold a new app | Rust, Bun, webview |
| `dev` | dev server + shell against it | the above |
| `build` | bundle the frontend, compile release | the above |
| `package` | build, then wrap in a platform installer | the above |
| `android build` | cross-compile and package an APK or AAB | NDK, SDK, JDK |
| `android run` | install and launch on a device | `adb`, a device |
| `ios build` | cross-compile and build the simulator app | Xcode |
| `ios run` | install and launch on a booted simulator | Xcode, simulator |
| `ios verify` | build, install, launch, assert the Rust entry is reached | Xcode |
| `docs` | validate `docs/src`, then build the website | Bun |
| `doctor` | check the toolchain and report what is missing | nothing |

## `init`

```bash
voussoir init              # scaffold into the current directory
voussoir init notes        # scaffold into ./notes
```

| Flag | Effect |
| --- | --- |
| `--name <n>` | app name; default is the directory name |
| `--force` | write into a non-empty directory |
| `--package <id>` | Android package name; default `com.voussoir.<app>` |
| `--mobile` | Android shell only, no desktop crate |

**What it writes** is documented in [How a project is laid out](layout.md). The
short version: `voussoir.json`, an entry HTML page, `src/`, and `native/` with the
shell crate and the commands crate. With `--mobile`, `native/` is Android-only.

**Why `--force` is not the default:** scaffolding overwrites `voussoir.json`,
`main.rs` and the command table. In a directory with an existing app that is a
silent loss of exactly the files you edited.

**A caution:** an empty directory is still a directory. `init` without a name in a
directory that contains files asks for `--force`, so `voussoir init` inside your
home directory does not scatter a project over it.

## `dev`

```bash
voussoir dev
voussoir dev --port 8080 --verbose
```

| Flag | Effect |
| --- | --- |
| `--port <n>` | dev server port; default 5177 |
| `--verbose` | record reloads, slow commands and refused navigations |

Starts the Bun dev server, then launches the shell against it, then watches both:
frontend changes are re-bundled and pushed over HMR, `native/commands` changes are
compiled to a shared library and swapped into the running process. Pressing Ctrl-C
stops both.

**What does not reload:** `native/src/main.rs`, `voussoir.json`, and `Cargo.toml`.
See [Hot reload in development](hot-reload.md).

**Port 5177** was chosen to avoid the common dev ports (3000, 5173, 8000). If it is
taken, the error names the port rather than failing obscurely; use `--port`.

## `build`

```bash
voussoir build
voussoir build --debug --size
```

| Flag | Effect |
| --- | --- |
| `--debug` | cargo debug profile instead of release |
| `--size` | print the resulting binary size |
| `--out <path>` | copy the binary somewhere |

Bundles the frontend, regenerates `native/src/assets.rs`, and compiles the release
binary. The output lands in `target/release/<name>`.

**`--debug` builds a different thing on purpose:** symbols, no LTO, no stripping.
Use it for a backtrace from a crash that only happens in release, not for shipping.

**`--out` exists for CI.** A workflow that needs to upload the binary should not
have to know the target triple's path layout.

## `package`

```bash
voussoir package
voussoir package --icon icon.png --identifier com.example.notes
```

| Flag | Effect |
| --- | --- |
| `--icon <path>` | 256×256 PNG, or `.icns` on macOS |
| `--identifier <id>` | reverse-DNS id; default derived from the name |

Produces `.AppImage` on Linux, a `.app` bundle on macOS, and a `.zip` containing
the `.exe` on Windows. See [Packaging](packaging.md) for what each contains and why
signing is left to you.

## `android`

```bash
voussoir android build
voussoir android build --abi x86_64 --api 30
voussoir android build --release
voussoir android build --aab
voussoir android run
```

| Flag | Effect |
| --- | --- |
| `--abi <abi>` | `arm64-v8a` (default), `x86_64`, `armeabi-v7a` |
| `--api <n>` | target API level |
| `--release` | signed release APK instead of debug |
| `--aab` | signed Android App Bundle for Play upload |

`--release` and `--aab` need a signing key; without one the command explains what is
missing rather than producing an unsigned artifact that cannot be installed.

`android run` installs and launches on the attached device or the running emulator.
See [Android and iOS](mobile.md).

## `ios`

```bash
voussoir ios build
voussoir ios build --target device
voussoir ios run
voussoir ios verify
```

| Flag | Effect |
| --- | --- |
| `--target sim` | simulator build (default) |
| `--target device` | device build, unsigned |

`ios verify` exists because a blank iOS window is almost always a wiring problem,
not a rendering problem. It builds, installs, launches, and asserts the Rust entry
was reached, which separates the two in one command.

## `docs`

```bash
voussoir docs
voussoir docs --check
```

| Flag | Effect |
| --- | --- |
| `--check` | validate `SUMMARY.md` and the links, build nothing |

Without `--check` it validates and then renders the site into `website/dist`.

**What `--check` validates:**

- every chapter in `SUMMARY.md` exists as a file,
- every `.md` file has an entry in the summary,
- no relative link points at a chapter the summary does not list.

The third happens for free in a renderer that fails on a missing page, and it is
the check that catches a typo in a filename before it becomes a 404. See
[Writing docs](writing-docs.md).

## `doctor`

```bash
voussoir doctor
voussoir doctor --json
```

| Flag | Effect |
| --- | --- |
| `--json` | machine-readable report |

Checks Rust, Bun, `pkg-config`, the platform webview, and in a project directory the
files `init` would have written. Each failure names the missing thing, because
"cargo not found" is actionable and "build failed" is not.

**Run it first when anything is odd.** It is faster than reading a compiler error
that is two layers downstream of a missing tool.

## Theory: what the CLI will never do

- **No `install` command.** A package is run in place: an AppImage is a file, a
  `.app` is dragged, a `.exe` is unzipped.
- **No `publish` command.** Uploading to a store, a registry or a server involves
  credentials the CLI should not have.
- **No `update` command.** Upgrading is re-running the installer, or pulling the
  repository. There is no self-replacing binary.
- **No global state.** Every command reads and writes the project directory and
  the standard toolchain locations, nothing else.

## Next

- [Installing Voussoir](install.md) — how the CLI gets on your machine.
- [Troubleshooting](troubleshooting.md) — when a command fails.
