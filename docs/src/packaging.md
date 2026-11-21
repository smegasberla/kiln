# Packaging

`voussoir package` builds the release binary and wraps it in the format users on
that platform actually install. It runs on the platform you are packaging for;
there is no cross-packaging.

## Theory: why packaging is per platform

There is no portable desktop installer format. Each OS decides what a "package"
is, and a tool that pretends otherwise produces something that runs but looks
foreign: no icon in the launcher, no uninstall entry, a warning on first launch.

So `package` does the one thing each platform expects, and nothing more:

| Platform | Output | How a user gets it |
| --- | --- | --- |
| Linux | `.AppImage`, one self-contained file, no root | `chmod +x`, run |
| macOS | `.app` bundle with a generated `Info.plist` | drag to Applications |
| Windows | the `.exe` in a `.zip`, WebView2 requirement noted | unzip, run |

```bash
voussoir package                      # default icon
voussoir package --icon icon.png      # 256x256 PNG, or .icns on macOS
voussoir package --identifier com.example.notes
```

An icon is optional. Without one the packager draws a default, so a build never
fails on missing artwork.

**Why an AppImage and not a `.deb`?** Because a `.deb` needs a repository, a
signing key and a root install, and most small apps have none of those. An
AppImage is a file the user can run, which matches what you can actually support.
A `.deb` can be added later; it is not a decision `package` has to make for you.

## What is inside

The binary embeds the frontend as an asset table (`native/src/assets.rs`), so the
package contains no `dist/` directory to lose and no filesystem read at startup.

The only runtime dependency is the OS webview:

- **Linux**: WebKitGTK 4.1
- **Windows**: WebView2, preinstalled on Windows 10 2020+ and 11
- **macOS**: WKWebView, part of the OS

`bun scripts/verify.ts` asserts the binary links the OS webview and embeds no JS
runtime, which is why "the shipped app contains no Node or Bun" is a measured
claim rather than a design intent.

## Theory: signing, and why this tool refuses

Code signing is not a build step: it is an identity claim backed by a certificate
you buy and a key you guard. A tool cannot invent one, and a package that *looks*
signed without being signed is worse than an honestly unsigned one, because it
teaches users to click through warnings.

So `voussoir package` prints the exact command for the platform instead:

- **macOS**: `codesign` then `notarytool submit`
- **Windows**: `signtool sign`
- **Linux**: an AppImage needs no signature; a distribution package would

The release workflow builds and uploads artifacts for a tag, and signs or
notarizes only when the corresponding secrets exist, so a fork without credentials
still produces a downloadable result.

**What users see without signing:**

| Platform | Unsigned behaviour |
| --- | --- |
| macOS | Gatekeeper refuses; user must right-click → Open once |
| Windows | SmartScreen warns until the binary earns reputation |
| Linux | no warning |

That is a real cost, and it is why macOS distribution effectively requires an
Apple Developer account.

## Theory: size

A release binary is a few megabytes, because the frontend is your code and the
engine is the OS's. The profile that gets it there:

```toml
[profile.release]
opt-level = "s"     # optimise for size
lto = true          # cross-crate inlining, then remove dead code
codegen-units = 1   # one unit, better optimisation
strip = true        # drop symbols
```

`voussoir build --size` prints the result, and `verify.ts` fails if it grows past
the recorded bound without the README being updated. That is a gate rather than a
trend chart: the useful question is "did this change make the app noticeably
bigger", and a threshold answers it.

## What to check before you ship

1. **`voussoir build`**, not `dev`: the release path is what users get.
2. **Run the packaged artifact**, not the binary: bundling changes paths and
   permissions.
3. **Open it on a clean machine or VM**: a missing webview shows up there and
   nowhere else.
4. **Check the icon at 16 px**: launchers and docks shrink it brutally.
5. **`voussoir build --size`** after the final change.
6. **Resize the window** to its smallest size; a webview has no responsive
   fallback beyond your CSS.

## What `package` does not do

- **No updater.** `update.check` tells you a version exists; fetching and applying
  it is yours, because a silent self-replacing binary is a decision an app should
  make deliberately.
- **No installation.** There is no `install` command: an AppImage is run in place,
  a `.app` is dragged, a `.exe` is unzipped.
- **No notarization.** See above; it needs your credentials.
- **No mobile packaging.** That is `voussoir android build` and
  `voussoir ios build`: [Android and iOS](mobile.md).

## Next

- [Android and iOS](mobile.md) — the two platforms with their own toolchains.
- [Compatibility policy](compatibility.md) — what an upgrade means for a shipped app.
