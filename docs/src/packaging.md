# Packaging

`voussoir package` builds the release binary and wraps it in the format users on that
platform actually install. It runs on the platform you are packaging for; there is
no cross-packaging.

| Platform | Output |
| --- | --- |
| Linux | `.AppImage`, one self-contained file, no root |
| macOS | `.app` bundle with a generated `Info.plist` |
| Windows | the `.exe` in a `.zip`, with the WebView2 requirement noted |

```bash
voussoir package                      # default icon
voussoir package --icon icon.png      # 256x256 PNG, or .icns on macOS
voussoir package --identifier com.example.notes
```

An icon is optional. Without one the packager draws a default, so a build never
fails on missing artwork.

## What is inside

The binary embeds the frontend as an asset table (`native/src/assets.rs`), so the
package contains no `dist/` directory to lose and no filesystem read at startup.
The only runtime dependency is the OS webview: WebKitGTK on Linux, WebView2 on
Windows (preinstalled on Windows 10 2020+ and 11), WKWebView on macOS.

`bun scripts/verify.ts` asserts the binary links the OS webview and embeds no JS
runtime, which is why "the shipped app contains no Node or Bun" is a measured claim
rather than a design intent.

## Signing

Voussoir does not attempt to sign. Signing needs credentials the tool cannot invent,
and a package that looks signed without being signed is worse than an honestly
unsigned one. `voussoir package` prints the exact command for the platform instead:

- macOS: `codesign` then `notarytool submit`
- Windows: `signtool sign`
- Linux: AppImage needs no signature; a distribution package would

CI builds and uploads artifacts on every run, and signs or notarizes only when the
corresponding secrets exist, so a fork without credentials still produces a
downloadable result.

## Size

A release binary is a few megabytes: `opt-level = "s"`, `lto = true`, `strip =
true`, `codegen-units = 1`. `voussoir build --size` prints the result, and
`verify.ts` fails if it grows past the recorded bound without the README being
updated.

## Next

- [Android and iOS](mobile.md) — the two platforms with their own toolchains.
