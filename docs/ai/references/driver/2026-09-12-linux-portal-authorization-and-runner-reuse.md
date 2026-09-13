# Linux Portal authorization and local Runner reuse

Status: implemented. The evidence sections describe the remaining desktop limits.
Scope: owner-approved Portal setup/persistence feature and reproduced daemon/SDK
lifecycle fixes. This builds on the [click modifiers contract](2026-09-11-click-modifiers-contract.md).

## Configuration and authorization

`auv::local` owns first-party local driver configuration. Direct invoke commands
and the `auv.core.local` Runner use `ai.moeru.auv`. Other library hosts can
configure their own `LocalDriver` app ID and state root. Bare `open_local()`
does not silently claim the AUV executable's identity.

Every input, ScreenCast, and clipboard Portal connection registers its configured
identity before creating Portal objects. Registration is connection-scoped, so a
new Runner registers again. Registration requires an installed desktop entry with the same ID.
AUV returns identity failures. It does not replace the identity with anonymous access.
This follows the [Registry contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html).

Setup commands run as the desktop user in that user's graphical D-Bus session:

```sh
auv doctor --portal-setup --json
auv doctor --portal-authorize --json
```

Setup installs `ai.moeru.auv.desktop` in the user's applications directory.
The entry records the absolute executable path with Desktop Entry quoting.
If the desktop service PATH lacks an SDK-bundled binary, GLib can reject `Exec=auv`.
Setup grants no rights.

Authorization opens input and capture sessions without input delivery.
It can show consent dialogs. The existing Portal response deadline is ten seconds.

If the executable moves, run the setup command again.

Token locations:

- Direct AUV commands: the platform-resolved AUV user state directory plus
  `portal` (normally `$XDG_STATE_HOME/auv/portal`).
- Daemon Runner: `<store-root>/runner-state/auv.core.local/portal`, preserving
  the existing daemon store boundary.
- To pre-authorize or diagnose that Runner store, pass
  `--portal-state-root <store-root>/runner-state/auv.core.local/portal` to doctor.

Keep the SDK's `storeRoot` stable between launches.

Different store roots have
separate token files. Input and capture retain their existing independent,
locked, atomic single-use token rotation with private directory/file modes.
The backend can still request consent after revocation or an unusable restore
token. The app ID change can require initial consent for previously anonymous
grants. This change does not migrate those grants or copy tokens between roots.

On a KDE desktop with a backend implementing its per-application allow rule:

```sh
auv doctor --portal-setup --portal-kde-allow --json
auv doctor --portal-kde-revoke --json
```

These explicit actions update only `ai.moeru.auv` in PermissionStore's
`kde-authorized` / `remote-desktop` rule. Revocation clears the allow rule. It does not terminate active sessions or revoke independent restore grants.
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

Tests reproduced two independent bugs before production changes:

1. A routed call without a Run created an ephemeral local Runner. Its final
   operation permit stopped the child immediately. The next call restarted it.
2. A second SDK `startAuv()` accepted health from an already listening daemon
   before its own child failed to bind. It returned a handle with the wrong
   process ownership relationship.

Lazy first-party local Runner creation now uses `unless-idle` with a five-minute
idle timeout. A class-specific creation lock makes concurrent first calls resolve
to one child. This lock does not serialize admitted calls. Existing
explicit lifecycle choices and custom-provider policies remain separate.
Active Run attachments retain the child. After their release, it can still serve
other Runs or calls during the idle period.

`startAuv()` starts an app-owned daemon.

Call it once per Node/Electron host. Then reuse `daemon.connect()` and `createAuv(connection)`.

Each launch supplies a fresh `--id` UUID. The SDK makes sure that every listener returns that `id` with a serving status. Health is
available through HTTP `GET /health` and gRPC `HealthService.Check`. Logging
and stdout formatting are independent. Without an explicit ID, the daemon
generates one for each bound instance.

An occupied
endpoint now fails. `connect()` attaches to an existing endpoint without spawning
or taking process ownership. Automatic shared connect-or-start remains deferred
until an explicit shared ownership contract is requested.

Runner ID/PID identify the worker. Run ID identifies an explicitly created
workflow. Two Runs can use the same Runner, and stopping one does not stop the
other.

Create the Run with `runs.create()`. Bind `runner({ runId, runnerClass })`.
When the workflow finishes, terminate that Run.

Omitting `runId` does not manufacture a Run
per RPC. Operations within one Run share its identity. Operation spans are the
separate concept for finer correlation. The daemon's control Runs remain
process-local state and do not promise complete durable tracing for every
low-level SDK call.

See [Run and Runner terms](../../../TERMS_AND_CONCEPTS.md)
and the [SDK usage](../../../../js/packages/sdk/README.md).

## Validation evidence (2026-09-12)

- **Behavior, macOS:** the real SDK daemon test covers concurrent cold calls and multiple connections.
  Repeated calls retain the local Runner PID. Two distinct Runs stop independently.
  An occupied endpoint rejects the new daemon while the original daemon remains healthy. Eight tests pass, with the Windows-only case skipped on macOS.
  A quiet-launcher regression makes sure that silent daemon stdout does not affect startup.
- **Behavior, isolated Linux D-Bus:** 68 driver tests pass.
  A private-bus fixture starts two client processes and observes Registry attribution on the exact Portal connection.
  KDE allow/revoke preserves the rule for another application. It requires `dbus-daemon` and runs with `--include-ignored`.
  Token tests make sure that a new store restores and rotates a durable private token.
  A Portal delivery fixture records actual SelectDevices/SelectSources options,
  restore-token replacement, modifier/button delivery order, immediate response
  signals, and cancellation closing the session without continuing to Start.
- **Behavior, Linux host `neko-gpu-1`:** built in an isolated `/tmp` checkout,
  installed the user desktop entry, and observed `identity_registered: true`.
  ScreenCast version 5 and Screenshot version 2 are present. RemoteDesktop is
  missing on the current Sway/wlr backend. No unattended input support is claimed.
- **Limits:** no real KDE consent-free input or compositor-reboot restoration
  receipt was obtained. The legacy Screenshot fallback is still interactive. Clipboard persistence and proactive recovery of existing capture/clipboard
  sessions after a Portal restart remain deferred. New processes/connections
  register again. This is not seamless live-session migration.

Additional check limits:

- `cargo test` and the focused Rust crate suites pass on macOS. The Linux daemon
  suite passes with one test thread. An initial parallel run hit an existing
  pairing-store lock contention test, outside the changed routing path.
- SDK ESLint passes. SDK typecheck still reports the three pre-existing
  `AbortSignal.any` ambient-type errors in `apis/auv/client.ts`,
  `apis/auv/driver.ts`, and `node/daemon.ts`. The relevant baseline expressions
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
pnpm --filter @auv-js/sdk exec vitest run src/node/daemon.test.ts
pnpm exec eslint js/packages/sdk/src/node/daemon.ts js/packages/sdk/src/node/daemon.test.ts
git diff --check
```

## Review follow-up: test quality and Portal libraries

The review removed the desktop-entry unit test.
Its assertions about formatter output did not establish that GLib can load the entry.
The review covered the six tests originally added by this PR. The other five cover validation,
durable token IO/private modes, D-Bus peer attribution/rule isolation, and actual
SDK/daemon behavior.

The live GLib/Registry probe remains the evidence for
the executable-path fix. No repository-wide test-quality count is claimed.

The Portal layer now uses
[ashpd 0.13.13](https://docs.rs/ashpd/0.13.13/ashpd/desktop/remote_desktop/struct.RemoteDesktop.html)
with its async-io backend. It owns Registry registration, ScreenCast,
RemoteDesktop, Clipboard, Screenshot, typed sessions/options, request tokens,
response decoding, and signal subscriptions. AUV retains its synchronous bounded
call boundary, token storage/rotation, logical geometry, PipeWire frames, and
permission reporting.

If startup fails, AUV closes the new sessions.
When the owner closes, clipboard listeners exit. The owner waits for their threads to stop.
Screenshot remains interactive and uses `url` for file-URI conversion.

Two narrow boundaries still use zbus:

- ashpd has no PermissionStore client, so KDE's per-application table uses a
  generated typed zbus proxy. This is not an alternative Portal session layer.
- ashpd 0.13 defaults `version()` to 1 for some property errors. Doctor reads the
  actual version property through ashpd's typed proxy to preserve failures as
  unknown rather than claiming availability. `PortalNotFound` maps to missing.

The review removed the request-token format test and two device/button constant tests.
The private-bus delivery fixture records observable calls instead. No new test
asserts ashpd's private representation or duplicates its protocol parser.

RustDesk at `e82dd12350479b848630d1e500c2f6870609a884` uses
[`dbus-codegen-rust` generated RemoteDesktop bindings](https://github.com/rustdesk/rustdesk/blob/e82dd12350479b848630d1e500c2f6870609a884/libs/scrap/src/wayland/remote_desktop_portal.rs)
and a [custom PipeWire/Portal session owner](https://github.com/rustdesk/rustdesk/blob/e82dd12350479b848630d1e500c2f6870609a884/libs/scrap/src/wayland/pipewire.rs),
not ashpd for this path. It reads ScreenCast's version to select persistence
support and retains its connection/session state in `RDP_SESSION_INFO`.

Health-ID follow-up validation: 22 daemon tests and eight SDK daemon tests pass on macOS.
HTTP/gRPC and multiple listeners return the same identity.
The tests cover distinct default IDs, explicit launcher IDs, silent stdout, and occupied endpoints. Full
repository ESLint passes. Health Protobuf generation, targeted lint and breaking
check against main pass.

Whole-workspace Buf lint still reports the existing
`MoveMouseStreamResponse` naming issue. Buf format reports existing reflection
option ordering. The change left both unrelated schemas intact.

ashpd migration validation: Linux driver tests pass (68 including two isolated
D-Bus fixtures). Linux `cargo clippy -p auv-driver-linux --all-targets
--all-features` completes with existing warnings in `atspi.rs`,
`session_test.rs`, and `window_test.rs`. Adding `-D warnings` fails on those
unchanged warnings. No new warning originates in the migrated Portal code.

The Linux CLI builds. A fresh `doctor --json` call with ashpd reports
`identity_registered: true`, ScreenCast v5, Screenshot v2, and RemoteDesktop
missing on `neko-gpu-1`. macOS `cargo fmt --check`, `cargo check`, default
`cargo test`, invoke help, full `pnpm lint`, and `git diff --check` pass.
