# Summary

A Kiln app is a Rust shell around a webview, with the frontend built by Bun. The
book goes from an empty directory to a signed installer, and spends its time on
the three things that decide whether an app feels finished: the command
contract, the security policy, and what happens when there is more than one
window.

If you only read one chapter, read "Commands & payloads". Everything else follows
from it.

- [Your first app](first-app.md)
  - [How a project is laid out](layout.md)
- [Commands & payloads](ipc-contract.md)
  - [Typed commands](typed-commands.md)
  - [Errors the page can branch on](errors.md)
- [Plugins](plugins.md)
- [Security model](security.md)
  - [The two origins](origins.md)
  - [Per-window allowlists](allowlists.md)
- [Multi-window](multi-window.md)
  - [Lifecycle hooks](lifecycle.md)
- [Hot reload in development](hot-reload.md)
- [Packaging](packaging.md)
  - [Android and iOS](mobile.md)
- [Operations](operations.md)
  - [Diagnostics](diagnostics.md)
  - [Benchmarks](benchmarks.md)
- [Platform support](platforms.md)
- [Compatibility policy](compatibility.md)
