# Compatibility policy

The crate is 0.x, so [Cargo's 0.x
rules](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)
already allow any minor bump to break. What Voussoir promises on top of that:

## Breaking changes

These require a 0.x bump and a CHANGELOG entry under its own heading:

- command names and the IPC envelope shape (`{"ok"|"error"}`)
- the public `App` and `AppHandle` signatures
- the `voussoir.json` schema
- the `window.voussoir` shim surface (`invoke`, `listen`, and their argument shapes)
- the meaning of an existing cargo feature

## Not breaking

- internal module layout
- new opt-in features, built-in commands, or `voussoir.json` keys (additive)
- patch bumps, which add no API change

## The shim protocol

The shim carries its own version, `voussoir.protocol`, separate from the crate version.
It exists so a frontend can feature-detect instead of probing for methods:

```ts
if (window.voussoir.protocol >= 2) {
  const open = await window.voussoir.windows();
}
if (window.voussoir.protocol >= 3) {
  const base = window.voussoir.pageOrigin;
}
```

The current version is **3**, which added `pageOrigin`. Version 2 added `windows()`,
`tray` and `menu`. Each guard names the version that introduced the member it
protects, because a page that asks for `>= 2` and then reads `pageOrigin` is
checking the wrong number on a version-2 shell.

## How a change is recorded

Every user-visible change is in `CHANGELOG.md` in Keep a Changelog format, so "is
this upgrade safe for my app?" is answerable from the changelog plus the lists
above. The version in `voussoir.json` and the version of the `voussoir` crate are separate
on purpose: an app does not have to adopt a new shell to keep its frontend working.
