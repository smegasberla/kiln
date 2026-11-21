# Installing Voussoir

Voussoir is a CLI plus a Rust crate. The CLI scaffolds projects, runs the dev
server, builds the release binary and wraps it in an installer. The crate is what
your app links.

There is no published package yet. `cargo install voussoir` and `npm i -g
voussoir` do not work, and any guide that suggests them is describing a future
release. Until then there are two ways in: the install script, or a checkout.

## The install script

One script installs the system webview, Git, Bun and Rust, builds the CLI from
source, and puts `voussoir` on `PATH`.

Linux, macOS and the BSDs:

```bash
curl -fsSL https://raw.githubusercontent.com/smegasberla/voussoir/master/install.sh | sh
```

Windows:

```powershell
irm https://raw.githubusercontent.com/smegasberla/voussoir/master/install.ps1 | iex
```

Both read the script from the default branch, so there is no download step and
nothing to keep up to date. Re-running the same command is how you upgrade:
every dependency that is already present is skipped, and the build takes
whatever is on the branch.

### Flags

| Unix | Windows | Effect |
| --- | --- | --- |
| `--prefix DIR` | `-Prefix DIR` | install location, default `$HOME/.local` (`%LOCALAPPDATA%\voussoir`) |
| `--no-deps` | `-NoDeps` | skip system package installation |
| `--repo URL` | `-Repo URL` | git URL to clone when not run from a checkout |

`VOUSSOIR_PREFIX` and `VOUSSOIR_REPO` do the same as the Unix flags, which is
handy in a Dockerfile where adding arguments to a piped script is awkward.

A failed system package install warns instead of aborting, so the run finishes
and `voussoir doctor` reports the remaining gap by name. The reason is
deliberate: on a machine where the webview is already present through another
channel, a package manager failure is not an error worth stopping for.

## From a checkout

If you have the repository, the scripts notice the tree they are in and build
that instead of cloning:

```bash
git clone https://github.com/smegasberla/voussoir.git
cd voussoir
./install.sh
```

The equivalent without the script, when the dependencies are already installed:

```bash
cd cli
bun install
bun run build     # -> cli/dist/voussoir, a compiled binary
```

`bun build --compile` embeds the Bun runtime into the CLI binary, so the result
needs neither Bun nor Node on `PATH`. That is a convenience for the developer
machine only: the apps the CLI builds never contain a JavaScript runtime.

## Checking the result

```bash
voussoir help        # the command list and every flag
voussoir doctor      # toolchain, webview and project checks
```

`doctor` is the command to run when something is missing rather than guessing.
It checks Rust, Bun, the platform webview, and in a project directory the files
`voussoir init` would have written. Its `--json` flag emits the same report for a
script to read.

## What gets installed

| Path | What |
| --- | --- |
| `$PREFIX/bin/voussoir` | the CLI binary |
| `%LOCALAPPDATA%\voussoir\bin\voussoir.exe` | the same, on Windows |
| `%LOCALAPPDATA%\voussoir\src` | the checkout Windows builds from |

On Unix nothing else is touched: no service, no autostart entry, and no shell
profile edit. The installer prints the `PATH` line and leaves the decision to
you. The Windows installer is the exception, because there is no equivalent of a
piped shell export, so it appends its `bin` directory to your user `PATH`
directly. New terminals pick that up; the current one needs the script to have
run in it, which piping to `iex` already does.

## Uninstalling

Remove the binary, then delete the `PATH` line the installer printed:

```bash
rm -f "$HOME/.local/bin/voussoir"
```

On Windows the installer owns a directory rather than a file, so one removal also
takes the source tree it left behind:

```powershell
Remove-Item "$env:LOCALAPPDATA\voussoir" -Recurse
```

Then drop that `bin` path from your user `PATH`, which the installer appended
without asking. App projects scaffolded in the meantime are ordinary directories
and are unaffected; delete those wherever you made them.
