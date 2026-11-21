# Android and iOS

Both mobile platforms reuse the command contract, registry, allowlist and security
policy. What differs is the transport and who owns the event loop, and therefore
which `voussoir` commands apply.

## Android

```bash
voussoir android build            # APK for arm64-v8a
voussoir android build --aab      # signed App Bundle for Play
voussoir android build --abi x86_64 --api 30
voussoir android run              # install and launch on a device or emulator
```

`voussoir init` scaffolds `android/` as a Gradle project, so nothing is generated at
build time except the Kotlin that wry writes (which is git-ignored).

The platform is different enough that it lives in its own crate, `voussoir-android`:

- **The activity owns the event loop.** Desktop creates one and runs it; Android is
  handed one from `onCreate` and drives it from the activity's callbacks.
- **`invoke` cannot use the custom protocol.** `WebResourceRequest` carries no body,
  so wry would deliver every payload as `null`. Android uses wry's `postMessage`
  handler and resolves the page's promise from Rust.
- **Tray is a notification.** It is posted once and cannot be retitled; a runtime
  update reports that rather than pretending it worked.

The envelope, allowlist and policy are shared through a transport seam, so a
command does not know which platform it is on. The instrumentation suite runs on an
emulator in CI, then launches the app with no instrumentation attached and fails on
a `FATAL EXCEPTION` or an `UnsatisfiedLinkError`.

## iOS

```bash
voussoir ios build                # simulator app
voussoir ios build --target device
voussoir ios run                  # install and launch on a booted simulator
voussoir ios verify               # build, install, launch, assert the Rust entry is reached
```

iOS keeps the custom-scheme transport: `WKURLSchemeHandler` intercepts
`voussoir://localhost` exactly as the desktop does, so the origin and the security
policy are the desktop ones. This is deliberate and is the one mobile platform where
the `http://` workaround is not needed.

Two differences matter in practice:

- **The first window is requested through the loop.** `tao` cannot build a window
  before `UIApplicationMain` runs, so the shell asks for the first window through
  the same path a command uses and gets it once the loop is up. The app code and
  the `on_ready` contract are identical.
- **Some features report unsupported.** Menus, dialogs, tray and the updater's
  endpoint fetch say so in their error rather than silently doing nothing. A page
  should branch on `voussoir.platform` for the affordance, not on the error.

## What is shared

| Concern | Desktop | Android | iOS |
| --- | --- | --- | --- |
| IPC transport | custom protocol | `postMessage` bridge | custom scheme |
| Envelope / errors | same | same | same |
| Allowlist | same | same | same |
| CSP / navigation policy | same | same | same |
| Command table | static (or hot in dev) | static | static |
| Hot reload | yes | no | no |

## Next

- [Platform support](platforms.md) — the tested matrix, per target.
