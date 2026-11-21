# Platform support

What is built, what is tested, and what is merely compiled. The distinction
matters: a cell that says `cargo check` means the code compiles for the target and
no window was opened.

| Platform | Build | Tests | Notes |
| --- | --- | --- | --- |
| Linux x86_64 | yes | yes | WebKitGTK 4.1 |
| Windows x86_64 | `cargo check` clean | yes (CI) | WebView2, preinstalled on Win10 2020+/11 |
| macOS x86_64 / arm64 | `cargo check` clean | yes (CI) | WKWebView |
| FreeBSD | CI (VM) | CI (VM) | GTK 3 + WebKit2GTK, same code path as Linux |
| NetBSD | `cargo check` | graph only | unverified at runtime |
| Android arm64 / x86_64 | yes | x86_64 only (emulator, CI) | `postMessage` transport, same commands and policy |
| iOS aarch64 (simulator) | yes | yes (simulator, CI) | custom-scheme transport, desktop origin and policy |

## BSD

BSD works because wry and tao already treat it as a webkitgtk/GTK platform. Two
things blocked it and both are fixed: tao's default `dbus` feature, and a
Linux-only `gtk` dependency. The remaining cost is that the `-sys` crates need GTK
and WebKit installed on the *target*, so BSD builds and runs on BSD rather than as
a cross-compile from Linux.

## Windows

The one real difference is the origin: WebView2 cannot intercept a custom scheme,
so pages are served from `http://voussoir.localhost`. See [The two origins](origins.md).

## Mobile

Android and iOS have their own chapter: [Android and iOS](mobile.md). The short
version is that the command contract is identical and the transport is not, and iOS
keeps the desktop security model while Android cannot.

## What is shared

Every platform uses the same command envelope, the same allowlist intersection, the
same CSP directives and the same navigation policy. That is the design goal: an app
is written once, and a platform difference is a transport detail or an
honestly-unsupported feature, never a second API.
