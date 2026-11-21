# Security model

The page is untrusted. Not because you wrote it, but because it renders content
you did not: markdown, remote images, an OAuth page, a link a user clicked. Voussoir's
policy assumes the worst about what runs inside the webview and gives Rust the
final say over what it can reach.

Four rules, each enforced by the shell and each proved by a test:

1. **The page is served from one origin, and only that origin.** Production serves
   `voussoir://localhost`; the security policy lists it and nothing else.
2. **Navigation leaves the app only where you allow it.** Anything else is refused
   by the shell and recorded, so a link does not silently become a full-page
   browser.
3. **Remote script, `eval` and remote fetch are blocked.** The CSP has no
   `unsafe-eval` and no remote script source, so a compromised dependency cannot
   evaluate a string into code.
4. **Commands are an allowlist, and a window can be narrower than the app.** An
   absent `commands` key means everything; a present one means exactly that list,
   intersected per window.

## What that buys you

A remote script that reaches the page still cannot call a command. Even a shell
injection into the webview is limited to the surface you registered. And a window
you deliberately gave a small surface cannot be talked into using the big one by
sending its own label: the shell reads the label from the webview the request
arrived on.

`bun scripts/verify.ts` runs this against a real window: it serves a hostile page,
points the app at it, and asserts the script did not execute, the fetch was
blocked, `eval` was blocked, and navigation did not leave the app.

## What it does not do

- It does not sandbox the Rust side. A command you wrote is trusted completely,
  which is why the interesting commands are the ones you register.
- It does not stop a webview engine CVE. Keep the platform webview updated; Voussoir
  uses the OS one, so that is your OS update path.
- It does not encrypt anything. `storage` is a JSON file in the platform data
  directory, readable by the user. Do not put secrets in it.

## Turning it off

You cannot turn these rules off. There is no config key that disables the CSP, no
API that widens navigation to an arbitrary origin, and no build flag that embeds a
JS runtime. If that sounds inflexible, it is on purpose: an app that needs to load
arbitrary remote pages should open the system browser, which is what the default
policy makes you do.

## Next

- [The two origins](origins.md) explains why there are two spellings and when each appears.
- [Per-window allowlists](allowlists.md) covers narrowing a window's surface.
