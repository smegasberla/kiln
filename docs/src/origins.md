# The two origins

A page's origin is one of the few things a webview will not let you lie about, and
Voussoir's policy is built on it. There are two spellings, and knowing which you are on
explains three otherwise confusing behaviours.

## Desktop

Pages are served from `voussoir://localhost`, a custom scheme the shell intercepts
itself. `window.location.origin` is `voussoir://localhost`, and the security policy
lists exactly that.

## Windows

WebView2 cannot intercept a custom scheme. wry therefore rewrites
`voussoir://localhost` to `http://voussoir.localhost` and intercepts that instead. Same
handler, same document, different origin string.

This is the one place a cross-platform app can notice the platform in its own
URLs. Two consequences:

- A page that hard-codes `voussoir://localhost/asset.png` in HTML gets a 404 on
  Windows. Use relative URLs, or read `voussoir.pageOrigin` when an absolute URL is
  genuinely needed: it carries the spelling this page was actually served from,
  so the same code is correct on every target.
- `voussoir.platform === "windows"` is available when a page genuinely needs the
  branch, but it should be rare: the shim's own `invoke` already talks to the right
  endpoint on both.

Earlier versions used a `http://localhost` spelling on Windows, which collided
with any local dev server and made the CSP ambiguous. The `voussoir.localhost` name is
reserved for this purpose, so it cannot collide with a real host.

## Android

`WebResourceRequest` carries no body, so the custom protocol cannot carry an
`invoke` payload at all. Android uses wry's `postMessage` bridge instead, and Rust
resolves the page's promise from the other side. The envelope, registry,
allowlist and policy are shared; only the transport differs.

The consequence for app code is zero: a command behaves the same. The consequence
for a page is that `fetch("voussoir://localhost/__voussoir/…")` is not the transport on
Android, so call `window.voussoir.invoke` rather than reaching for the endpoint by
hand.

## iOS

`WKURLSchemeHandler` intercepts `voussoir://localhost` the way the desktop does, so
the origin and the policy are the desktop ones. This is deliberate: it is the one
mobile platform where the insecure-by-default `http://` workaround is not needed.

## What to write in a page

Relative URLs, and `window.voussoir.invoke`. Both work on every platform, and neither
requires the page to know which origin it is on. When you cannot avoid an absolute
URL, build it from `voussoir.pageOrigin` rather than from a literal; that member exists
so the rewritten spelling stays an implementation detail instead of something every
page has to know.

`voussoir.pageOrigin` arrived in shim protocol 3, so the honest feature test is
`voussoir.protocol >= 3` with a relative URL as the fallback.
