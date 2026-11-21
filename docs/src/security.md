# Security model

The page is treated as untrusted. Not because you wrote it, but because it renders
content you did not: markdown, remote images, an OAuth page, a link a user
clicked. Voussoir's policy assumes the worst about what runs inside the webview and
gives Rust the final say over what it can reach.

Four rules, each enforced by the shell and each proved by a test.

## Theory: what a threat model is, and why this app needs one

A threat model is a short list of "what could go wrong, and what stops it". You do
not need a hostile-user story to benefit from one: the interesting attack surface
of a desktop app is usually *the content it displays on someone's behalf*.

Concretely, three ways a Voussoir app could be attacked:

1. **A remote page replaces your UI.** If navigation were free, a link or a
   redirect could put a foreign page in your window, with no address bar and no
   way back. The user cannot tell it is not you.
2. **A remote script runs in your context.** If the CSP allowed remote scripts,
   a compromised dependency or an injected tag could call every command your app
   registered, including file access.
3. **A narrow window escalates.** If a window rendering untrusted content could
   call the whole app, the isolation you intended is decoration.

The four rules answer exactly those three, in order.

## Rule 1: one origin, and only that origin

Production serves `voussoir://localhost`. The policy lists that origin (and the
platform's rewritten spelling) and nothing else. A request for any other origin
is refused before it becomes a document.

**Why an origin and not a file path?** Because an origin is one of the few things
a webview will not let a page lie about. Paths can be crafted; `location.origin`
is assigned by the engine.

## Rule 2: navigation leaves the app only where you allow it

A link to `https://example.com` does not navigate. It is refused and recorded:

```
voussoir: [security] t+10233ms navigation refused dev_origin=null url=https://example.com/
```

The alternative (silently allowing it) strands the user in a page that is not
your app, in a window with no chrome.

**What to do instead:** `target="_blank"` hands the URL to the OS browser, which
is where a web page belongs. See [The frontend](frontend.md) for the patterns.

## Rule 3: no remote script, no eval, no remote fetch

The Content-Security-Policy served with every response:

```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self';
object-src 'none'; base-uri 'none'; form-action 'none'
```

Read it as a list of what the page *can* do:

- `script-src 'self'`: only scripts you bundled. No CDN, no inline script.
- `connect-src 'self'`: `fetch` can only reach your own origin. Network calls
  belong in Rust, where they are a command you can see and gate.
- `style-src 'self' 'unsafe-inline'`: inline styles are allowed because bundled
  CSS emits `<style>`, and a style tag cannot reach the bridge. Note the
  asymmetry: inline *styles* are fine, inline *scripts* are not.
- `object-src 'none'`, `base-uri 'none'`, `form-action 'none'`: plugins, `<base>`
  hijacking and form posts to elsewhere are closed off.

Development relaxes this: Bun's HMR client uses inline script, an eval-based
module loader and a `ws:` connection. That relaxation is scoped to the dev origin
and never applies to a shipped app.

**Why no `unsafe-eval`?** Because `eval` turns any string into code. If a
compromised dependency can evaluate a string, the CSP is the only thing between
it and your commands, and a policy with `unsafe-eval` is not a policy.

## Rule 4: commands are an allowlist, and a window can be narrower

An absent `commands` key in `voussoir.json` means everything registered. A
present one means exactly that list. A `windows` entry narrows one label further,
and the intersection **can only remove**:

```json
{
  "commands": ["greet", "save", "delete"],
  "windows": { "preview": { "commands": ["greet"] } }
}
```

Here `preview` can call `greet` and nothing else. The rule is an intersection:
the effective list is the window's list **and** the global list. So a `windows`
entry can never grant a command that the global `commands` list withheld, which
means a config file cannot be used to escalate a window past the app's own
surface.

**Where the label comes from matters.** The shell reads the calling window's label
from the webview the request arrived on, never from the payload. A page cannot
claim another window's permissions because there is no field to claim them in.

## What this buys you

A remote script that somehow reaches the page still cannot call a command. Even a
shell injection into the webview is limited to the surface you registered. And a
window you deliberately gave a small surface cannot be talked into using the big
one.

`bun scripts/verify.ts` runs this against a real window: it serves a hostile page,
points the app at it, and asserts the script did not execute, the fetch was
blocked, `eval` was blocked, and navigation did not leave the app. Those are
measurements, not intentions.

## What it does not do

- **It does not sandbox the Rust side.** A command you wrote is trusted
  completely. The interesting authority in the app is the set of commands you
  register, which is why the allowlist is the lever.
- **It does not stop a webview engine CVE.** Voussoir uses the OS webview, so your
  OS update path is your security patch path.
- **It does not encrypt anything.** `storage` is a JSON file in the platform data
  directory, readable by the user. See [Storage](storage.md).
- **It does not validate payloads.** A typed command does, but that is a
  correctness tool, not a security boundary.

## Turning it off

You cannot. There is no config key that disables the CSP, no API that widens
navigation to an arbitrary origin, and no build flag that embeds a JS runtime.

That is a deliberate trade, and it is worth being honest about the cost: an app
that genuinely needs to load arbitrary remote pages (a browser, a docs viewer)
cannot use the default policy and must open the system browser instead. If that
is your app, Voussoir is the wrong tool, and finding that out here is better than
finding it out in review.

## Next

- [The two origins](origins.md) — why there are two spellings and when each appears.
- [Per-window allowlists](allowlists.md) — narrowing a window's surface.
