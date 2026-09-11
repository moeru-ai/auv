# Linux Portal authorization and local Runner reuse

Status: implemented; evidence levels and remaining desktop limits below.
Scope: owner-approved Portal setup/persistence feature and reproduced daemon/SDK
lifecycle fixes. This builds on the [click modifiers contract](2026-09-11-click-modifiers-contract.md).

## Configuration and authorization

`auv::local` owns first-party local driver configuration. Direct invoke commands
and the `auv.core.local` Runner use `ai.moeru.auv`. Other library hosts can
configure their own `LocalDriver` app ID and state root; bare `open_local()`
does not silently claim the AUV executable's identity.

Every input, ScreenCast, and clipboard Portal connection registers its configured
identity before creating Portal objects. Registration is connection-scoped, so a
new Runner registers again. A matching installed desktop entry is required.
Explicit identity failure is returned, not replaced with anonymous access.
This follows the [Registry contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html).

Setup commands run as the desktop user in that user's graphical D-Bus session:

```sh
auv doctor --portal-setup --json
auv doctor --portal-authorize --json
```

Setup installs `ai.moeru.auv.desktop` in the user's applications directory.
It records the installing executable's absolute path with Desktop Entry quoting:
GLib can reject `Exec=auv` when an SDK-bundled binary is absent from the desktop
service's PATH. Rerun setup after moving the executable. Setup grants no rights;
authorize opens input and capture sessions without injecting input and can show
consent dialogs. The existing Portal response deadline is ten seconds.

Token locations:

- Direct AUV commands: the platform-resolved AUV user state directory plus
  `portal` (normally `$XDG_STATE_HOME/auv/portal`).
- Daemon Runner: `<store-root>/runner-state/auv.core.local/portal`, preserving
  the existing daemon store boundary.
- To pre-authorize or diagnose that Runner store, pass
  `--portal-state-root <store-root>/runner-state/auv.core.local/portal` to doctor.

Keep the SDK's `storeRoot` stable between launches. Different store roots have
separate token files. Input and capture retain their existing independent,
locked, atomic single-use token rotation with private directory/file modes.
The backend may still request consent after revocation or an unusable restore
token. The app ID change can require initial consent for previously anonymous
grants; this change does not migrate those grants or copy tokens between roots.

On a KDE desktop with a backend implementing its per-application allow rule:

```sh
auv doctor --portal-setup --portal-kde-allow --json
auv doctor --portal-kde-revoke --json
```

These explicit actions update only `ai.moeru.auv` in PermissionStore's
`kde-authorized` / `remote-desktop` rule. Revocation clears the allow rule;
it does not terminate active sessions or revoke independent restore grants.
Ordinary calls never modify this rule. The behavior is KDE-specific, as used
by [Sunshine](https://github.com/LizardByte/Sunshine/blob/master/docs/troubleshooting.md#portal-token-issues)
and defined in the [KDE backend](https://github.com/KDE/xdg-desktop-portal-kde/blob/master/src/remotedesktop.cpp).

Doctor distinguishes interface availability, identity registration, token file
presence, and the KDE stored rule. Desktop environment detection is not proof
that a particular backend or version honors that rule. None of these observations
alone establishes restored authorization or delivered input. Interface presence
now maps to `unknown` in the shared permission probe, rather than `granted`.
Diagnostics and PermissionStore method calls have bounded replies.

## Daemon, Runner, and Run identities

Two independent bugs were reproduced before production changes:

1. A routed call without a Run created an ephemeral local Runner. Its final
   operation permit stopped the child immediately; the next call restarted it.
2. A second SDK `startAuv()` could accept health from an already listening daemon
   before its own child failed to bind. It returned a handle with the wrong
   process ownership relationship.

Lazy first-party local Runner creation now uses `unless-idle` with a five-minute
idle timeout. A class-specific creation lock makes concurrent first calls resolve
to one child. Actual admitted calls are not serialized by this lock. Existing
explicit lifecycle choices and custom-provider policies remain separate.
Active Run attachments retain the child; after their release, it can still serve
other Runs or calls during the idle period.

`startAuv()` starts an app-owned daemon. Call it once per Node/Electron host,
then reuse `daemon.connect()` and `createAuv(connection)`. It first waits for that
child's post-bind readiness announcement, then checks every listener. An occupied
endpoint now fails. `connect()` attaches to an existing endpoint without spawning
or taking process ownership. Automatic shared connect-or-start remains deferred
until an explicit shared ownership contract is requested.

Runner ID/PID identify the worker. Run ID identifies an explicitly created
workflow. Two Runs can use the same Runner, and stopping one does not stop the
other. Call `runs.create()`, bind `runner({ runId, runnerClass })`, and terminate
that Run when the workflow finishes. Omitting `runId` does not manufacture a Run
per RPC. Operations within one Run share its identity; operation spans are the
separate concept for finer correlation. The daemon's control Runs remain
process-local state and do not promise complete durable tracing for every
low-level SDK call. See [Run and Runner terms](../../../TERMS_AND_CONCEPTS.md)
and the [SDK usage](../../../../js/packages/sdk/README.md).

## Validation evidence (2026-09-12)

- **Behavior, macOS:** the real SDK daemon test covers concurrent cold calls,
  multiple connections, stable local Runner PID across repeated calls, two
  distinct Run IDs, independent Run termination, and rejection of an occupied
  endpoint while the original daemon remains healthy. Seven tests pass; the
  Windows-only case is skipped on macOS.
- **Behavior, isolated Linux D-Bus:** 70 driver tests pass, including a private-bus
  fixture that starts two client processes, checks Registry attribution on the
  exact Portal caller connection, and verifies KDE allow/revoke preserves another
  application's rule. It requires `dbus-daemon` and runs with `--include-ignored`.
  Token tests verify a new store restores and rotates a durable private token.
- **Behavior, Linux host `neko-gpu-1`:** built in an isolated `/tmp` checkout,
  installed the user desktop entry, and observed `identity_registered: true`.
  ScreenCast version 5 and Screenshot version 2 are present. RemoteDesktop is
  missing on the current Sway/wlr backend; no unattended input support is claimed.
- **Limits:** no real KDE consent-free input or compositor-reboot restoration
  receipt was obtained. The legacy Screenshot fallback is still interactive;
  clipboard persistence and proactive recovery of existing capture/clipboard
  sessions after a Portal restart remain deferred. New processes/connections
  register again; this is not seamless live-session migration.

Additional check limits:

- `cargo test` and the focused Rust crate suites pass on macOS. The Linux daemon
  suite passes with one test thread; an initial parallel run hit an existing
  pairing-store lock contention test, outside the changed routing path.
- SDK ESLint passes. SDK typecheck still reports the three pre-existing
  `AbortSignal.any` ambient-type errors in `apis/auv/client.ts`,
  `apis/auv/driver.ts`, and `node/daemon.ts`; the relevant baseline expressions
  are unchanged.
- Windows SSH (`luoling-windows-11`, port 22) timed out. A macOS-to-MSVC daemon
  check stopped in `aws-lc-sys`'s C build, so it does not establish Windows
  daemon compilation or behavior for this change.

Validation commands:

```sh
cargo fmt --check
cargo check
cargo test
cargo test -p auv-daemon -p auv-driver-linux -p auv-driver -p auv -p auv-cli-invoke
# Linux host, with dbus-daemon available:
cargo test -p auv-driver-linux -- --include-ignored
cargo test -p auv local::tests
pnpm --filter @auv-js/sdk exec vitest run src/node/daemon.test.ts
pnpm exec eslint js/packages/sdk/src/node/daemon.ts js/packages/sdk/src/node/daemon.test.ts
git diff --check
```
