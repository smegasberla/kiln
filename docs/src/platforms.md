# Platform support

What is built, what is tested, and what is merely compiled. The distinction
matters: a cell that says `cargo check` means the code compiles for the target and
no window was opened.

## Theory: why the table is honest about the gaps

Every cross-platform project has a table like this, and most of them are marketing:
a row means "we have users there". This one means something narrower, and it is
worth understanding the three levels it uses, because they are not the same claim:

| Cell | Means |
| --- | --- |
| `yes` | built and run, on that platform, by CI or by hand |
| `cargo check` | compiles for the target; no window was opened |
| `graph only` | the dependency graph resolves; nothing was compiled |
| a caveat | the thing works, but not in the way the cell would imply |

The reason to draw the line this carefully: a compile-only target is where
runtime bugs live. `cargo check` passing means the types line up, not that the
webview opens.

| Platform | Build | Tests | Notes |
| --- | --- | --- | --- |
| Linux x86_64 | yes | yes | WebKitGTK 4.1 |
| Windows x86_64 | `cargo check` clean | yes (CI) | WebView2, preinstalled on Win10 2020+/11 |
| macOS x86_64 / arm64 | `cargo check` clean | yes (CI) | WKWebView |
| FreeBSD | CI (VM) | CI (VM) | GTK 3 + WebKit2GTK, same code path as Linux |
| NetBSD | `cargo check` | graph only | unverified at runtime |
| Android arm64 / x86_64 | yes | x86_64 only (emulator, CI) | `postMessage` transport, same commands and policy |
| iOS aarch64 (simulator) | yes | yes (simulator, CI) | custom-scheme transport, desktop origin and policy |

## Theory: why BSD works at all

BSD works because `wry` and `tao` already treat it as a webkitgtk/GTK platform:
the same code path as Linux, with a different package manager. Two things blocked
it and both are fixed: `tao`'s default `dbus` feature, and a Linux-only `gtk`
dependency.

The remaining cost is real and worth knowing: the `-sys` crates need GTK and
WebKit installed on the **target**, so BSD builds and runs on BSD rather than as a
cross-compile from Linux. That is why the CI cell says "VM" rather than
"cross-compiled".

## Windows

The one real difference is the origin: WebView2 cannot intercept a custom scheme,
so pages are served from `http://voussoir.localhost`. See
[The two origins](origins.md).

Two consequences worth planning for:

- An absolute URL built from a literal is wrong on Windows only. Use relative
  URLs or `pageOrigin`.
- WebView2 is preinstalled on Windows 10 (2020+) and 11, but not on older Windows
  or on Windows Server without the Evergreen runtime. If you support those, ship
  the bootstrapper.

## Mobile

Android and iOS have their own chapter: [Android and iOS](mobile.md). The short
version is that the command contract is identical and the transport is not, and
iOS keeps the desktop security model while Android cannot (its `WebResourceRequest`
carries no body, so the payload travels over `postMessage` instead).

## Theory: what "same everywhere" actually means

Every platform uses the same command envelope, the same allowlist intersection,
the same CSP directives and the same navigation policy. That is the design goal:
an app is written once, and a platform difference is a transport detail or an
honestly-unsupported feature, never a second API.

What is *not* the same, and cannot be:

| Concern | Varies because |
| --- | --- |
| Rendering | four webview engines, four CSS engines |
| Origin spelling | WebView2's interception limits |
| Menus and tray | the OS decides what those are |
| Fonts, scrollbars, form controls | the platform draws them |
| Dynamic loading | mobile forbids it; hot reload is desktop-only |

Design for the last table, not against it. Test the CSS on the engines you ship
to; do not assume one rendering matches another.

## How to add a platform

1. `cargo check --target <triple>` and fix the `-sys` crates.
2. Open a window and load the default app: `voussoir dev` on that machine.
3. Run `bun scripts/verify.ts`: it asserts the policy and the transport.
4. Add CI, then update this table with what you actually observed.

Step 2 is the one people skip, and it is the one that finds the platform bugs.

## Next

- [Android and iOS](mobile.md) — the mobile toolchains.
- [Compatibility policy](compatibility.md) — what is promised across versions.
