# AIRI bundled AUV daemon research

Status: research plus macOS implementation handoff. This is not a complete
computer-use replacement contract.

Date: 2026-08-14

Repositories inspected:

- AUV `1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a`
- AIRI `0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa`

Scope:

- Ship the platform-matching `auv` executable as part of AIRI's Electron
  application.
- Let the Electron main process own one app-local `auv serve` daemon and decide
  which computer-use tools are visible to a model or renderer.
- Cover build, bundle, signing, notarization, update, child lifecycle, and
  platform risks.
- Exclude CDP and browser automation.

Evidence level: source audit plus official Electron, electron-builder, Node,
and Apple documentation; focused AIRI unit tests and type/lint checks; one
local arm64 AIRI package and update ZIP signed with the MOERU AI Developer ID;
update-manifest hash and payload inspection; and a live packaged-binary
Run/Runner/`display.list` probe. Apple notarization submission, Intel hardware,
an installed update replacement, and clean-TCC behavior were not tested.

## Implementation update

The first macOS slice now exists in the AIRI working tree:

- AIRI pins AUV revision `1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a`,
  builds it for each macOS matrix target, checks its Mach-O architecture, and
  stages it under an ignored `.sidecars` directory.
- electron-builder copies it to `Contents/MacOS/auv`, lists it in
  `mac.binaries`, signs it before the outer application, and includes the final
  application in the existing notarization flow.
- An Injeca-owned Electron main-process manager starts `auv serve`, creates one
  Run and one `auv.core.local` Runner, reuses them for calls, and stops the
  Runner, Run, and daemon from `onAppBeforeQuit`.
- AIRI's existing MCP aggregation seam now accepts first-party providers. The
  built-in `auv` provider currently exposes only `display_list`, `window_list`,
  and `probe_permissions`. It exposes no raw daemon endpoint or arbitrary AUV
  operation.

Because `@auv-js/sdk` and `@auv-js/api-client` are not published, this slice uses the
same pinned `auv` executable as both daemon and typed-command adapter. Each
operation is an allowlisted CLI invocation routed to the Electron-owned daemon
with the selected Device and Run. Replace this adapter with the matching
`@auv-js/sdk` package after it is published; the manager and tool-provider ownership
boundary does not need to change.

### AUV npm distribution update (2026-08-17)

AUV now has a source-complete npm distribution path under
`js/packages/cli`; this is implementation evidence, not evidence that version
`0.0.6` has been published or installed from npm. The package keeps the
napi-rs template's root-package plus platform-package layout:

- `@auv-js/cli` exports the NAPI binding, `binaryPath()`, and the `auv` npm
  command shim;
- five optional packages cover macOS arm64/x64, glibc Linux arm64/x64, and
  Windows x64;
- each platform package carries both its `.node` binding and the matching
  standalone `auv` executable;
- the existing AUV release matrix builds both artifacts, while the npm
  assembly step verifies the release archive SHA-256 and prefers the notarized
  macOS archive before publishing;
- installation uses npm optional dependencies instead of a network-fetching
  `postinstall` hook.

This removes the need for Electron applications to invent a GitHub download
protocol. It does not remove Electron's bundle boundary: the application must
still copy `binaryPath()` outside `app.asar`, sign the copied nested executable
as part of the final macOS app, resolve the app-owned absolute path at runtime,
and let the main process own its lifecycle. AIRI's existing
`Contents/MacOS/auv` implementation remains the reference shape for that
staging step.

Local signed-package evidence:

- electron-builder reported `signing additional user-defined binaries` for
  `Contents/MacOS/auv` before signing `AIRI.app`;
- strict verification passed for both the helper and the outer app;
- the helper had Team ID `433DLLA855`, Hardened Runtime, and arm64 architecture;
- AIRI's actual manager used that packaged helper to start a daemon, create a
  Run and ready local Runner, and complete `display.list`;
- `app.probePermissions` reported Accessibility, Apple Events, ScreenCaptureKit,
  and Screen Recording as granted on the development host. This is not a clean
  TCC-state test.

Signing, notarization, and updater follow-up evidence:

- strict deep verification passed for the complete arm64 application after the
  updater ZIP was built, and strict verification also passed for the bundled
  AUV helper extracted from that ZIP;
- the generated `latest-arm64-mac.yml` SHA-512 matched the actual
  `AIRI-0.11.3-arm64-mac.zip`, whose payload contained both
  `Contents/MacOS/auv` and `Contents/Resources/licenses/auv/LICENSE`;
- the release workflows now require `stapler validate` and Gatekeeper `spctl`
  assessment after electron-builder, so a skipped notarization or missing
  ticket fails the macOS release job;
- the ordinary release workflow also verifies that each updater ZIP contains
  AUV and its license and is named by the architecture-specific updater
  manifest;
- the locally built application was deliberately built with notarization
  disabled because no Apple notarization credentials were available. As
  expected, `codesign --verify --deep --strict` passed while `stapler validate`
  reported no ticket and Gatekeeper rejected it as `Unnotarized Developer ID`;
- updater unit and manifest coverage passed 37 focused tests. A packaged stable
  lane check resolved the current GitHub release and reached the expected
  no-update result. The updater test harness was corrected so packaged builds
  no longer pretend to use the development-only `UPDATE_SERVER_URL` override
  and generic initialization logs cannot produce false green results.

This validates the signed update payload boundary, but it is not evidence that
Squirrel replaced an installed AIRI build. That final update lifecycle check
requires two published, notarized application versions. The existing global
`before-quit` path does await AIRI lifecycle hooks and Injeca shutdown before
the second `app.quit()`, so the AUV Runner, Run, and daemon are on the updater
quit path by construction; a real replacement test remains the behavioral
proof.

## Executive conclusion

The proposed topology is viable and is the right near-term meaning of “AUV is
built into AIRI”:

```text
AIRI Electron main process
  -> AIRI-owned first-party tool projection
  -> one app-owned AUV supervisor
  -> bundled absolute-path `auv serve` child
  -> caller-local Unix socket on macOS
  -> allowlisted calls through the same bundled `auv` binary
```

It does not require Rust and Electron to share a process. AUV already owns the
hard part of this boundary: `startAuv()` starts `auv serve`, waits on typed
health, exposes connection facts, and provides idempotent graceful-to-forced
shutdown. AIRI already has two useful precedents: a packaged Godot sidecar
resolved from `process.resourcesPath`, and app-wide child cleanup through its
`onAppBeforeQuit` lifecycle.

The work is not yet “just add one `extraResources` entry.” The minimum useful
integration still needs:

1. a pinned, checksummed AUV binary acquisition/staging step for every AIRI
   target;
2. platform-specific bundle placement, including explicit nested-code signing
   for macOS;
3. an AIRI main-process AUV supervisor using the matching command or SDK
   revision;
4. AIRI-owned capability/tool exposure rather than a raw daemon or raw process
   bridge to renderers;
5. packaged smoke tests for startup, update shutdown, permissions, and actual
   Driver operations;
6. separate closure of Windows and Linux distribution blockers described
   below.

For the current release matrices, macOS x64/arm64 is the closest product slice.
Linux x64/arm64 has matching AUV artifacts but still needs runtime-library and
Flatpak/portal validation. Windows x64 now has a release archive and a
first-party local Driver Runner, but still needs executable signing, installer
staging, SmartScreen, and packaged live-Driver evidence.

## What already exists

### AIRI already bundles and supervises a native sidecar

AIRI's electron-builder config has `asar: true` and copies the exported Godot
stage through `extraResources` into `resources/godot-stage`. The packaged main
process resolves that executable from `process.resourcesPath`; development mode
instead resolves a configured local engine. See AIRI
[`electron-builder.config.ts`](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/apps/stage-tamagotchi/electron-builder.config.ts#L96-L106)
and
[`godot-stage/index.ts`](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/apps/stage-tamagotchi/src/main/services/airi/godot-stage/index.ts#L293-L370).

The Godot manager also demonstrates the required main-process ownership shape:
absolute executable path, direct `spawn`, captured stdout/stderr, startup
readiness, status, expected-versus-unexpected exit handling, graceful shutdown,
forced termination, and registration with `onAppBeforeQuit`. See
[`godot-stage/index.ts`](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/apps/stage-tamagotchi/src/main/services/airi/godot-stage/index.ts#L740-L863)
and its
[`setupGodotStageManager`](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/apps/stage-tamagotchi/src/main/services/airi/godot-stage/index.ts#L901-L920).

AIRI's root exit flow prevents the first `before-quit`, awaits application
hooks and Injeca cleanup, closes logging, then quits again. This is suitable for
awaiting `daemon.stop()` during ordinary quit and `autoUpdater.quitAndInstall()`
flows. See
[`src/main/index.ts`](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/apps/stage-tamagotchi/src/main/index.ts#L292-L358).
The existing MCP stdio manager independently proves that AIRI already treats
external tool processes as app-owned sessions and closes all of them from the
same hook. See
[`mcp-servers/index.ts`](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/apps/stage-tamagotchi/src/main/services/airi/mcp-servers/index.ts#L151-L200)
and
[`setupMcpStdioManager`](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/apps/stage-tamagotchi/src/main/services/airi/mcp-servers/index.ts#L441-L458).

This means AUV does not need a new generic Electron child framework for the
first slice. It needs one cohesive AUV supervisor, shaped like the existing
app-wide managers but using `@auv-js/sdk`'s accepted daemon lifecycle.

### AUV already has the app-owned daemon lifecycle

`auv-js/node` exports `startAuv()`. Its returned handle contains the child PID,
binary path, endpoints, store root, serializable connection defaults, an
`exited` promise, `connect()`, and idempotent `stop()`. The implementation:

- invokes exactly `auv serve`, not `auv daemon`;
- supplies explicit listener and store arguments;
- waits until every listener reports typed health;
- fails startup if the process exits or the deadline expires;
- sends `SIGINT` for graceful shutdown and escalates to `SIGKILL` after a
  deadline.

See AUV
[`daemon.ts`](https://github.com/moeru-ai/auv/blob/1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a/js/packages/sdk/src/node/daemon.ts#L18-L109),
[`startAuv`](https://github.com/moeru-ai/auv/blob/1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a/js/packages/sdk/src/node/daemon.ts#L122-L284),
and
[`serveArguments` / `stopChild`](https://github.com/moeru-ai/auv/blob/1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a/js/packages/sdk/src/node/daemon.ts#L302-L352).

On macOS and Linux, an empty listener list becomes a caller-local Unix socket
under the chosen store root. On Windows it becomes loopback HTTP. For AIRI's
in-process Node client, Unix is the narrower default: no port collision, no
browser bearer enrollment, and no need to publish discovery. AIRI should pass
`noDiscovery: true` and keep the socket/connection handle inside the main
process.

The foreground Rust command also owns Ctrl-C cancellation and daemon shutdown,
and on Unix it launches the same installed `auv` executable in an internal
first-party Runner role. See
[`serve.rs`](https://github.com/moeru-ai/auv/blob/1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a/crates/auv-cli/src/commands/serve.rs#L73-L142).
Consequently, bundling one executable is sufficient on macOS/Linux: the daemon
does not require a separately packaged local-Runner executable.

### Five AIRI/AUV release targets now match exactly

AIRI currently builds these five desktop targets:

- `x86_64-apple-darwin`;
- `aarch64-apple-darwin`;
- `x86_64-unknown-linux-gnu`;
- `aarch64-unknown-linux-gnu`;
- `x86_64-pc-windows-msvc`.

See the AIRI
[`release-tamagotchi.yml` matrix](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/.github/workflows/release-tamagotchi.yml#L52-L89).

AUV's audited revision emitted the first four target-specific archives. The
current workflow also emits Windows x64, calculates SHA-256 files for all five,
signs both macOS binaries with Hardened Runtime, and submits the macOS
executables to Apple notarization. See AUV
[`release.yml`](https://github.com/moeru-ai/auv/blob/1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a/.github/workflows/release.yml#L31-L115)
and its
[`notarize-macos` job](https://github.com/moeru-ai/auv/blob/1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a/.github/workflows/release.yml#L159-L207).

This is strong artifact-production evidence, but not proof that a downloaded
AUV archive can be inserted unchanged into AIRI's final signed app. AIRI must
still sign and notarize the complete final nested-code graph and test the final
DMG/installed app.

## Recommended bundle design

### Keep AUV outside ASAR

Electron's ASAR is a virtual, read-only filesystem. Electron documents that
`spawn` cannot reliably execute a binary inside ASAR; only `execFile` gets the
special extraction behavior. AUV must therefore be a real executable outside
`app.asar`. See the official
[`ASAR Archives` limitations](https://www.electronjs.org/docs/latest/tutorial/asar-archives#executing-binaries-inside-asar-archive).

electron-builder documents that `extraResources` copies outside ASAR into
macOS `Contents/Resources` or Windows/Linux `resources`, while `extraFiles`
copies into macOS `Contents` or the Windows/Linux application root. See
[`Application Contents`](https://www.electron.build/docs/contents/#extra-files-and-resources).

Recommended installed layout:

```text
macOS:   AIRI.app/Contents/MacOS/auv
Windows: <AIRI install>/resources/bin/auv.exe
Linux:   <AIRI install>/resources/bin/auv
```

For Windows and Linux, use platform-scoped `extraResources` and resolve
`join(process.resourcesPath, "bin", executableName)`.

For macOS, AIRI's current Godot precedent puts an application bundle beneath
Resources and works as evidence for `extraResources/process.resourcesPath`.
That does not make Resources the best location for a bare Mach-O helper. Apple's
code-signing guidance names `Contents/MacOS` and `Contents/Helpers` as standard
nested-code locations and warns that code placed where data is expected may be
sealed as both code and a resource. Prefer platform-scoped `extraFiles` to
`Contents/MacOS/auv`, then explicitly include the helper in electron-builder's
macOS additional-binary signing configuration (`mac.binaries`) for the pinned
builder version. Resolve it from the main process by walking from
`process.resourcesPath` (`Contents/Resources`) to `Contents/MacOS/auv`.

Apple requires nested code to be signed inside-out before the outer app is
signed. See Apple's
[`Code Signing Guide: Nested Code`](https://developer.apple.com/library/archive/technotes/tn2206/_index.html#//apple_ref/doc/uid/DTS40007919-CH1-TNTAG201)
and
[`Creating distribution-signed code`](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac).
electron-builder supports additional macOS binaries and performs signing before
building the distributable; see its
[`macOS configuration`](https://www.electron.build/mac/)
and
[`build lifecycle`](https://www.electron.build/docs/features/build-lifecycle/).

If AIRI deliberately keeps AUV under `Contents/Resources/bin` to match its
existing sidecar layout, that is an alternative requiring explicit nested
signing and final strict signature verification. It must not rely on
`extraResources` alone or on the standalone AUV archive having once been
notarized.

### Stage one pinned artifact per matrix job

AIRI should not download “latest” during electron-builder execution and should
not select a binary at runtime. A deterministic preparation step should run
before `electron-builder`:

1. pin an AUV release/revision in AIRI source;
2. derive the AUV target from the existing AIRI matrix target, not from the
   build host implicitly;
3. download or build the matching archive;
4. verify its committed/pinned SHA-256;
5. extract only `auv` or `auv.exe` to an ignored staging directory;
6. verify architecture and executable startup;
7. let electron-builder copy that exact staged file into the application.

Conceptual staging layout:

```text
apps/stage-tamagotchi/.sidecars/auv/
  x86_64-apple-darwin/auv
  aarch64-apple-darwin/auv
  x86_64-unknown-linux-gnu/auv
  aarch64-unknown-linux-gnu/auv
  x86_64-pc-windows-msvc/auv.exe   # not available yet
```

There are two defensible producers:

- consume a checksummed, immutable AUV release archive; or
- checkout a pinned AUV revision and build it natively inside each AIRI matrix
  job.

Release consumption gives cleaner separation and faster AIRI builds. Building
inside AIRI gives exact source provenance but adds Rust, submodules, Swift
bridge, Linux system libraries, and signing work to an already heavy workflow.
Do not copy a developer's `target/release/auv` into a release build.

The standalone macOS AUV artifact and AIRI app may also be signed by different
certificate material even if both repositories belong to the same
organization. The AIRI packaging pipeline should therefore treat the staged
file as nested input and ensure the final helper carries AIRI's intended Team
ID and Hardened Runtime signature before the outer app is notarized.

### Lock the SDK and binary together

AIRI currently has no `@auv-js/sdk` or `@auv-js/*` dependency in its package or lock
file. Adding the binary without adding a compatible Node SDK would leave AIRI
with either ad-hoc HTTP calls or a generic MCP subprocess, neither of which
matches the intended typed integration.

The AUV daemon API is explicitly marked experimental and not a long-term
compatibility promise in
[`runner.proto`](https://github.com/moeru-ai/auv/blob/1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a/proto/auv/api/daemon/v1/runner.proto#L1-L5).
The `@auv-js/sdk` package also depends on workspace-generated API code. See
[`auv-js/package.json`](https://github.com/moeru-ai/auv/blob/1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a/js/packages/sdk/package.json#L1-L50).

Once AIRI consumes `@auv-js/sdk`, it must consume the SDK and `auv` binary from the
same AUV release/revision. Until those packages are published, using the same
pinned CLI for daemon control and allowlisted operation calls keeps the wire
contract on one revision without copying an ad-hoc HTTP client into AIRI. A
packaged test must still create a Run/Runner route and execute one harmless
Driver read. `auv --help` or process health alone cannot detect schema drift.

As of the research date, public npm lookups for both `@auv-js/sdk` and
`@auv-js/api-client` return `404 Not Found`. The source package is structured
for publication, but AIRI cannot yet add a normal registry dependency. The
release seam must either publish both packages together or attach tested,
versioned package tarballs to the same AUV release. A raw Git dependency is a
weaker product path because AIRI would inherit AUV's workspace generation and
build assumptions instead of consuming immutable SDK output.

## Recommended Electron supervisor

The supervisor should be an AIRI main-process module, not a renderer API and
not another user-editable entry in `mcp.json`.

Its ownership should be:

```text
AuvDaemonManager
  owns: binary resolution, daemon process, Run, Runner, and command adapter
  exposes internally: status, selected typed operations, start/stop/restart
  does not expose: arbitrary command, child handle, raw Unix socket, bearer
                   secret, generic invoke to renderer
```

Suggested packaged start facts:

```text
binaryPath: platform-specific absolute installed path
command:    auv serve
storeRoot:  <AIRI userData>/auv
listeners:  []               # app-local Unix socket on macOS/Linux
noDiscovery: true
pairingStore: absent         # main-process caller-local authority only
```

In development, use an explicit `AUV_BINARY_PATH` (or similarly named
development-only setting) and validate that it is an executable file. Falling
back to `auv` on `PATH` is convenient for contributors but must not be the
packaged behavior. The packaged resolver should branch on `app.isPackaged`, as
the Godot manager already does, and fail with the exact expected path and
platform/architecture if the sidecar is missing.

Lifecycle requirements:

- Create only one manager after the single-instance guard and `app.whenReady()`.
- Start lazily when the computer-use module is enabled, or eagerly when the
  owning Injeca provider is requested; do not start one daemon per window.
- Await Run and Runner readiness before publishing tools as available.
- Observe `daemon.exited`; distinguish requested shutdown from crash and remove
  tools immediately on unexpected exit.
- Keep persistent signature, permission, or schema failures visible instead of
  creating a restart loop.
- Register `daemon.stop()` with `onAppBeforeQuit` so update installation and
  normal quit stop the old process before application files are replaced.
- Stop the owned Runner and Run before or together with daemon shutdown.
- Preserve daemon stderr/recent startup output in AIRI logs with secrets
  redacted.

The landed manager covers normal graceful-to-forced shutdown, but this does not
establish parent-death semantics for Electron crashes, OS shutdown, or forced
termination. An attached Node child is not a cross-platform orphan guarantee.
Product hardening needs a separate bounded slice: Unix parent-death behavior or
heartbeat, Windows Job Object once Windows exists, stale endpoint cleanup, and
startup reclamation. `daemonIdleTimeoutSeconds` is useful as a fallback but is
not evidence that a live orphaned Runner will exit.

### AIRI should own tool visibility

The daemon is an execution substrate. AIRI should continue owning model-facing
tool names, descriptions, approval policy, feature flags, and contextual
exposure.

A good boundary is:

```text
AIRI model/tool registry
  -> AIRI approval and policy
  -> AuvComputerAdapter (typed calls only)
  -> app-owned @auv-js/sdk client
```

Electron renderers should receive only capability-level Eventa/IPC methods and
status needed by the UI. Do not expose Node `spawn`, the AUV child handle, raw
`ipcRenderer`, an unrestricted daemon proxy, or a generic “call any AUV
operation” bridge. Electron's security guidance recommends one method per IPC
message and sender validation rather than exposing raw IPC; see
[`Context Isolation`](https://www.electronjs.org/docs/latest/tutorial/context-isolation#security-considerations)
and
[`Security`](https://www.electronjs.org/docs/latest/tutorial/security).

This also allows AIRI to expose no computer-use tools when permission is
missing, a feature is disabled, the daemon is unhealthy, or the current model
is not approved, without changing which capabilities AUV implements.

## Signing, notarization, update, and permission risks

### macOS signing and notarization

AIRI already enables Hardened Runtime and electron-builder notarization, and
its CI supplies a signing certificate plus Apple notarization credentials. See
AIRI
[`electron-builder.config.ts`](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/apps/stage-tamagotchi/electron-builder.config.ts#L137-L226)
and
[`release-tamagotchi.yml`](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/.github/workflows/release-tamagotchi.yml#L185-L197).

Adding AUV changes the nested code graph. Release acceptance must inspect the
final AIRI application, not just the input helper:

```text
codesign --verify --deep --strict --verbose=2 AIRI.app
codesign -dv --verbose=4 AIRI.app/Contents/MacOS/auv
spctl --assess --type exec --verbose=4 AIRI.app
xcrun stapler validate AIRI.app
```

Also run `file` or `lipo -info` on both the Electron executable and AUV helper
to prevent x64/arm64 mixing. electron-builder's official notarization guidance
likewise recommends strict signature, Gatekeeper, and stapler checks; see
[`macOS Notarization`](https://www.electron.build/docs/notarization/#testing-notarization).

The final signed bundle must be immutable. AUV state, discovery, socket,
pairing, Runs, and artifacts belong below AIRI's writable application-data
directory, never beside the bundled helper.

### macOS TCC is a packaged live-probe blocker

AUV's macOS AX and capture code runs in a child executable. Apple's AX API asks
whether the *current process* is a trusted accessibility client, and
ScreenCaptureKit requires user-granted Screen Recording permission. See
[`AXIsProcessTrustedWithOptions`](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions)
and Apple's
[`ScreenCaptureKit sample`](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos).

Source inspection does not prove how macOS will present or persist TCC consent
for this particular signed nested CLI launched by Electron. Do not claim that
AUV automatically inherits AIRI's Accessibility or Screen Recording grant, and
do not claim the opposite. This must be tested with the final signed,
notarized, installed AIRI app on a clean TCC state.

The acceptance probe must record:

- which identity appears in System Settings and permission prompts: AIRI,
  `auv`, or another responsible-process attribution;
- whether granting Accessibility allows AX reads and input delivery from the
  nested child after relaunch;
- whether granting Screen Recording allows full-display, region, and window
  capture after the required restart;
- behavior before grant, after denial, after grant, after an AIRI update, and
  after moving/reinstalling the app;
- whether x64 and arm64 signed builds behave the same.

Until that evidence exists, macOS packaging is technically implementable but
permission UX and support claims remain blocked.

### Updates should update AIRI and AUV atomically

Because AUV is included through `extraFiles`/`extraResources`, it naturally
becomes part of AIRI's DMG/NSIS/deb/rpm/AppImage/Steam application payload. It
should not run its own updater. AIRI's updater must:

1. stop the old app-owned daemon in `before-quit`;
2. install the new AIRI payload including the matching AUV and `@auv-js/sdk`;
3. let the new AIRI process start the new daemon;
4. re-run the typed compatibility/health probe before exposing tools.

This avoids UI/SDK/daemon version drift. It also means any release process that
re-signs or mutates the installer after electron-builder must regenerate update
hash metadata, as AIRI already does for its signed Windows installer.

Steam is a separate build workflow with only Windows x64, macOS arm64, and
Linux x64 depots. It copies unpacked Electron output after building, so the AUV
sidecar will be included if staged before electron-builder, but that workflow
needs the same acquisition, macOS nested signing, and validation steps as the
ordinary release. It cannot inherit changes made only to
`release-tamagotchi.yml`.

## Platform artifact matrix

| AIRI target | AUV release artifact | `auv serve` + first-party local Runner | Bundle readiness | Required closure |
| --- | --- | --- | --- | --- |
| macOS x64 | Yes: `auv-x86_64-apple-darwin.tar.gz` | Yes: Unix executable Runner | Close | Stage pinned binary; place as standard nested code; sign/notarize final AIRI app; run clean-TCC AX/capture/input probe. |
| macOS arm64 | Yes: `auv-aarch64-apple-darwin.tar.gz` | Yes: Unix executable Runner | Close | Same as macOS x64; verify native arm64 rather than Rosetta fallback. |
| Linux x64 GNU | Yes | Yes: Unix executable Runner | Conditional | Inspect ELF runtime dependencies; declare/bundle them for deb/rpm/AppImage; verify X11/Wayland portal input/capture and Flatpak permissions. |
| Linux arm64 GNU | Yes | Yes: Unix executable Runner | Conditional | Same as Linux x64 plus native arm64 dependency and portal testing. |
| Windows x64 MSVC | Yes: `auv-x86_64-pc-windows-msvc.zip`; Authenticode not yet evidenced | Yes: loopback daemon plus named-pipe executable Runner, tested 2026-08-16 | Conditional | Sign the executable, then validate staging, NSIS, SmartScreen, and live Driver behavior. |

### Windows runtime and archive gaps closed; signing evidence remains

The 2026-08-16 AUV slice enabled the first-party local Driver Runner and custom
executable Runners on Windows. The daemon uses one local-only named pipe per
Runner. An automated test routes `invoke display.list` through this boundary.
See
[`2026-08-16-windows-local-runner-ipc-handoff.md`](2026-08-16-windows-local-runner-ipc-handoff.md).

AUV now produces a Windows x64 archive. AIRI integration still lacks
Authenticode evidence for the nested executable, final installer staging, and
packaged live Driver evidence. Do not infer bundle readiness from an unsigned
archive or source-tree tests.

Once implemented, the Windows executable must be included before NSIS signing
and verified in both unpacked output and the final signed installer. AIRI's
current SignPath flow signs the installer after electron-builder and regenerates
updater hashes; the sidecar's own Authenticode status must be included in the
acceptance evidence rather than inferred from installer signing.

### Linux needs distribution, portal, and Flatpak evidence

AUV's Linux release jobs install native development packages including XCB,
XRandR, D-Bus, PipeWire, Wayland, xkbcommon, EGL, Leptonica, and Tesseract, then
archive only the `auv` executable. This proves build inputs, not that the
resulting ELF is self-contained on AIRI's supported distributions. See
[`release.yml`](https://github.com/moeru-ai/auv/blob/1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a/.github/workflows/release.yml#L57-L79).

Before bundling Linux releases, inspect `ldd`/`readelf -d` on both target
artifacts and either:

- declare matching deb/rpm dependencies;
- bundle permitted shared libraries with a controlled loader/RPATH strategy;
- or produce a deliberately portable AUV artifact.

AppImage and Flatpak need separate live probes. AIRI's Flatpak manifest copies
the complete `linux-*-unpacked` directory, so a resource sidecar will travel
with it, but the manifest's runtime, portal talks, filesystem access, and
zypak-wrapped Electron process do not prove that the AUV child can use the same
portal/session facilities. See AIRI
[`ai.moeru.airi.flatpak.yml`](https://github.com/moeru-ai/airi/blob/0ef3a26f7ded46ad831ff3a7e2248dee468bb6fa/apps/stage-tamagotchi/ai.moeru.airi.flatpak.yml#L1-L43).

Linux support claims should be split by package format and desktop session:
deb/rpm/AppImage/Flatpak × X11/Wayland, with capture, input, window, AX/AT-SPI,
and persistence tested as distinct evidence.

## Development and packaged path contract

| Concern | Development | Packaged application |
| --- | --- | --- |
| Binary source | Explicit `AUV_BINARY_PATH` or documented local `target/debug/auv`; optional `PATH` fallback | Immutable platform/arch-matched sidecar included by electron-builder |
| Binary resolution | Validate supplied absolute path and executable bit | Derive absolute installed path from `process.resourcesPath`; never search `PATH` |
| Store root | Isolated developer/test directory | AIRI-owned writable app-data subdirectory, never app resources |
| Listener | Isolated Unix socket or explicit test port | Caller-local Unix socket on macOS/Linux; no discovery |
| SDK | Workspace/pinned development package | Same pinned AUV release/revision as bundled binary |
| Signing | May be unsigned/ad hoc; not permission evidence | Final Developer ID-signed/notarized app and helper on macOS; final signed installer/helper evidence on Windows |
| Test meaning | Integration and developer ergonomics | Product behavior, TCC, updater, package dependencies, and support evidence |

Tests must not silently swap between system-installed AUV and the bundled AUV.
Log the resolved path, target architecture, AUV version/revision, endpoints, and
store root at startup without logging credentials.

## Concrete implementation slices

The owner approved the macOS integration and signing slice. Later platform and
capability slices remain unapproved follow-up candidates.

### Slice 1: macOS arm64 packaged daemon proof

Implemented end-to-end proof:

- pin one AUV revision and use its CLI as the temporary matching adapter;
- stage it before electron-builder;
- place it at `Contents/MacOS/auv` and explicitly sign it as nested code;
- add one main-process manager owning daemon, Run, and local Runner;
- register stop with `onAppBeforeQuit`;
- expose status internally but no generic renderer API;
- build and sign AIRI locally;
- verify strict signatures, Runner readiness, and one Driver read.

Notarization, Gatekeeper, stapling, and clean-state TCC behavior remain for the
release workflow or a signed release-candidate probe.

This answers the highest-risk questions before multiplying CI matrix work.

### Slice 2: AIRI-owned tool projection

- map a deliberately small set of typed AUV operations into existing AIRI
  approval/tool policy;
- publish tools only while daemon and permission facts permit them;
- test enable/disable, daemon crash, denied permission, and app quit;
- keep raw daemon endpoints and arbitrary invocation out of renderer IPC.

### Slice 3: complete macOS matrix and updater

- add x64 acquisition and checks;
- exercise Intel hardware or an accepted native runner;
- test update from an AIRI version carrying AUV N to one carrying AUV N+1;
- verify the old daemon exits and the new SDK/binary pair becomes healthy.

### Slice 4: Linux packaging evidence

- close dynamic-library packaging;
- validate deb/rpm and then AppImage/Flatpak separately;
- record X11/Wayland/portal evidence before making public support claims.

### Slice 5: Windows AUV runtime

This belongs first in AUV, not AIRI packaging:

- [x] implement and test the Windows first-party local Runner transport;
- publish signed `x86_64-pc-windows-msvc` AUV artifacts;
- then add AIRI staging, supervisor, NSIS, updater, and Driver behavior tests.

## Acceptance checklist

Build and provenance:

- [x] AUV binary source and its CLI control adapter are pinned to one revision.
- [ ] SHA-256 is checked before extraction.
- [x] Every macOS AIRI job selects the exact target architecture.
- [x] Missing or wrong-architecture sidecar fails staging or packaging.
- [ ] License and third-party notices for the bundled binary are included.

Package layout:

- [x] AUV is outside ASAR and resolved by an absolute path.
- [x] macOS helper is in a standard nested-code location with explicit signing
      evidence.
- [x] Runtime state is directed to AIRI's `userData` directory.
- [ ] Linux executable permissions survive packaging and Flatpak copying.

Lifecycle:

- [x] Exactly one daemon is owned per AIRI main process.
- [x] Run and local Runner readiness gate tool publication.
- [x] Unexpected exit changes manager status and the provider omits unavailable
      tools.
- [x] Normal quit and `quitAndInstall` run the shared shutdown hook.
- [ ] Crash/orphan behavior has an explicit test and mitigation.
- [x] Daemon output streams into AIRI logging without an in-memory buffer.

Behavior:

- [ ] Typed health succeeds against the bundled binary.
- [x] The first-party local Runner reaches ready state.
- [x] A Run and Runner route can be created.
- [x] `display.list` succeeds through the signed packaged helper.
- [ ] Tool enable/disable remains AIRI policy, not renderer access to raw AUV.

macOS release:

- [x] Local signed app passes `codesign --deep --strict`.
- [x] Helper Team ID, Hardened Runtime, and arm64 architecture are inspected.
- [ ] Final app passes `spctl` and stapler validation.
- [ ] Accessibility and Screen Recording are tested from clean TCC state.
- [ ] Permission behavior after relaunch and update is recorded.

Linux release:

- [ ] ELF shared-library closure is known for x64 and arm64.
- [ ] Package dependencies or bundled libraries are explicit.
- [ ] X11/Wayland and deb/rpm/AppImage/Flatpak evidence are separated.

Windows release:

- [ ] AUV publishes a Windows artifact.
- [ ] `auv.core.local` is present through the non-Unix Runner transport.
- [ ] Helper and installer signing are both verified.

## Bottom line

Electron-managed `auv serve` is not a speculative architecture. Both
repositories already contain its two halves: AIRI knows how to ship and own a
sidecar, while AUV exposes an Electron/Node-oriented app-owned daemon API.

The honest distance is:

- **macOS arm64 packaging proof:** landed and locally validated with the signed
  packaged helper; notarization and clean-TCC evidence remain;
- **macOS x64/arm64 product path:** CI now builds both architectures; native x64
  evidence, notarization/stapling, and an updater test remain;
- **Linux x64/arm64:** matching artifacts exist, but distribution dependencies
  and portal/package-format evidence remain;
- **Windows x64:** runtime and archive production exist; executable signing,
  Electron/NSIS staging, SmartScreen, and packaged Driver evidence remain.

The daemon should replace duplicated execution infrastructure incrementally.
AIRI should remain the owner of which computer-use tools exist for a model,
when they are visible, and which approval policy guards them.

## Computer-operation capability gap

This section compares only native computer operation. CDP, browser DOM,
terminal/PTY, coding workflows, secret access, and web search are outside the
comparison.

### The process topology has one daemon and one lazy local Runner

“One AIRI-managed daemon” does not mean that only one AUV process exists while
an operation is active. The implemented Unix topology is:

```text
AIRI Electron main
  -> bundled auv serve process
    -> same bundled auv executable in internal local-Driver Runner mode
      -> auv-driver-macos or auv-driver-linux
```

The daemon spawns the local Runner over an inherited socketpair and owns its
health and lifetime. This is still one AIRI-owned daemon boundary and one
shipped executable. It is a good sidecar design; reducing it to an in-process
Rust/Node binding is not required for AIRI integration and would cross a much
larger FFI and lifecycle seam.

### Current public-surface comparison

The relevant AUV boundary is the typed `RunnerClient` in
[`auv-js/src/apis/auv/driver.ts`](https://github.com/moeru-ai/auv/blob/1ca775f957dd11b96ddc73f77e8e5f3c38d8bd4a/js/packages/sdk/src/apis/auv/driver.ts),
not AUV's generic MCP `invoke` tool. AIRI can call this client in Electron main
and project only approved product tools.

| Native capability | AIRI computer-use-mcp today | AUV through `@auv-js/sdk` today | Gap classification |
| --- | --- | --- | --- |
| Display enumeration | `display_enumerate` | Direct: `displays.list()` | Ready |
| Identify display/local point | `display_identify_point` | Display frames are available; AIRI can compute containment and local coordinates | AIRI-side composition |
| Window list and target resolution | `desktop_observe_windows` | Direct: `windows.list()` / `windows.resolve()`; AUV also carries bundle ID and stable operation-local window refs | Ready |
| Full display, region, and window screenshot | `desktop_screenshot` | Direct capture methods returning raw RGBA | API exists, but the message-size and image-projection blocker below must close |
| OCR / find visible text | No equivalent native public tool in the basic executor loop | Direct display/window OCR and captured-frame recognition | AUV is ahead |
| Open application | `desktop_open_app` | No public app-launch operation | Missing operation |
| Focus/activate application | `desktop_focus_app` | macOS `activateBundleId()` with foreground verification | Ready on macOS after AIRI app-name/bundle-ID mapping |
| Screen/window click | left/right/middle and click count | Screen and window click support count/interval, but the public contract is left-button only | Partial; button must become part of the owned click contract |
| Pointer movement | Internal pointer trace used by click | Direct planned and streaming mouse motion | AUV is ahead |
| Type/paste text | `desktop_type_text`, optional pre-click | Direct type and clipboard-preserving paste; pre-click is two typed calls | Ready by composition |
| Key press/chord | Broad macOS key-code table including arrows | Public string API exists; macOS currently accepts a smaller special-key set plus supported single-character shortcuts | Partial; arrows/navigation/function-key parity is missing on macOS |
| Horizontal/vertical scroll at a point | `desktop_scroll` | Implemented in Rust on macOS/Linux/Windows, including window-targeted paths, but absent from Proto and `@auv-js/sdk` | Implemented substrate; missing public surface |
| Wait | `desktop_wait` | No daemon call is needed | AIRI-side timer |
| Clipboard read/write | `clipboard_read_text` / `clipboard_write_text` | Implemented by platform Driver APIs, but absent from Proto and `@auv-js/sdk` | Implemented substrate; missing public surface |
| AX snapshot | `accessibility_snapshot` | Public API exposes only `focusText()`; Rust has tree capture, path focus, and text readback | Implemented substrate; missing public surface and schema decisions |
| AX find element | `accessibility_find_element` | Can be an AIRI query over a public AUV snapshot; no separate daemon search is required | Blocked by snapshot surface |
| Unified observe / candidate IDs | `desktop_observe` merges screenshot, windows, AX, and optional Chrome candidates | No cross-source target-candidate API | Keep as an AIRI adapter, not a new AUV candidate schema |
| Click candidate from fresh observation | `desktop_click_target` enforces snapshot freshness and resolves a candidate to Chrome or OS input | Raw window/screen click exists; no AIRI candidate or freshness policy | Keep resolution/freshness in AIRI, deliver input through AUV |
| Permissions/capability preflight | `desktop_get_capabilities` | macOS permission probe plus RunnerClass/service reflection | AIRI-side projection; add platform evidence before claiming support |
| Approval queue, action budget, task state | AIRI-owned policy/state tools | AUV has Run/Runner resources and typed delivery evidence, not AIRI's approval UX or task policy | Keep in AIRI |

The practical reading is:

- The basic visual loop is close: display/window observation, capture, focus,
  left click, type/paste, key chords, pointer movement, OCR, and client-side wait
  already have typed calls.
- Scroll, clipboard, and AX observation are not new platform implementations;
  they mainly need owned Proto/Runner/JavaScript surfaces and regression tests.
- App launch, non-left click, and macOS navigation-key coverage are real
  operation gaps rather than JavaScript-only omissions.
- AIRI's unified grounding, snapshot freshness, approval, tool budget, and
  model-visible tool selection should not move into AUV. They are product
  policy above the Driver seam.

### AX is substantial underneath but deliberately narrow in the public API

The answer to “AUV probably does not have many AX APIs yet” depends on the
layer:

- Public daemon/`@auv-js/sdk`: only `FocusText` is exposed.
- Capability-oriented macOS Driver: tree capture, node-path focus, text focus,
  and text readback exist.
- Native Swift boundary: tree capture, arbitrary AX action dispatch, focus,
  and node inspection exist.

That is useful substrate, but it is not yet a product AX API. In particular,
AUV's current `ObservedAxNode` contains path, role/subrole, title,
description/help, identifier, placeholder, value, focus, and bounds, but it
does not carry AIRI's `enabled` fact. AIRI currently marks `enabled === false`
nodes as non-interactable. Reusing the existing grounding logic without adding
that producer fact would risk presenting disabled controls as candidates.

The smallest parity contract is therefore one read-only AX snapshot RPC with
explicit truncation/limits and the facts AIRI's grounding consumes. AIRI can
rebuild the hierarchy from depth/path and implement `find_element` locally.
General AX press/set-value/action APIs are not required to match AIRI's current
native behavior, because its existing AX candidates ultimately fall back to
coordinate clicks. They should remain a later capability decision rather than
expanding the archived AX-copilot vertical.

### Full-resolution capture currently hits a JavaScript transport limit

AUV sends captures as tightly packed, uncompressed RGBA bytes. A 1920x1080
frame is about 7.9 MiB before protobuf overhead; Retina frames are larger.
The current `@auv-js/sdk` gRPC transport constructs `@grpc/grpc-js`'s `Client`
without channel options. The pinned `@grpc/grpc-js` 1.14.4 default maximum
receive message length is 4 MiB.

Therefore a normal full-display capture over the preferred Unix gRPC transport
can exceed the client limit even though the local Runner server allows large
messages. Before AIRI treats screenshot as product-ready, `@auv-js/sdk` needs an
owned receive-size setting and a regression test above 4 MiB. The AIRI adapter
also needs to turn raw RGBA into the image/tool-result representation expected
by the model without duplicating screenshots unnecessarily.

This is a small implementation slice but a release blocker for the visual
loop. Process health and low-resolution capture tests would not catch it.

### Minimum backlog for “capability is not far apart” on macOS

The following is a bounded parity backlog, not approval to implement the
slices:

1. Fix large-frame transport and add AIRI's RGBA-to-image result projection.
2. Expose typed global/window scroll through Proto, local Runner, and `@auv-js/sdk`.
3. Expose text clipboard read/write through the same owned layers.
4. Expose a read-only macOS AX snapshot carrying truncation and `enabled`, then
   implement AIRI-side find/ranking over it.
5. Add mouse-button selection and complete the macOS navigation-key set used by
   AIRI.
6. Add an app-launch operation distinct from activate/focus.
7. Build the AIRI `desktop_observe`/`desktop_click_target` adapter over AUV
   observations and input while retaining AIRI freshness, approval, and state.

Items 1-6 close the lower execution/observation layer. Item 7 reconnects AIRI's
existing product semantics. None requires CDP, a generic AUV MCP subprocess, or
an in-process Rust binding.

### Recommended replacement boundary in AIRI

Do not initially “replace computer-use-mcp with the AUV MCP server.” Replace
its lower native backend and then retire the standalone stdio process only
after the AIRI-owned upper layer has another home:

```text
keep in AIRI Electron main
  tool catalog and enable/disable policy
  approval queue and action budgets
  task/session state and candidate freshness
  desktop_observe candidate projection
  desktop_click_target resolution
  overlay/presentation state
        |
        v
replace native execution with
  AuvComputerAdapter -> @auv-js/sdk -> auv serve -> local Driver Runner
```

AIRI's renderer currently obtains MCP tools through two main-process Eventa
calls: list tools and call a qualified tool. The least disruptive product seam
is a first-party Electron tool provider merged beside user-configured stdio MCP
providers. It can publish an `auv` or temporary `computer_use` namespace while
enabled and route calls directly to `AuvComputerAdapter`; it should not add the
bundled daemon to the user's `mcp.json`.

This preserves Electron control over exposure and keeps the renderer unaware
of process paths, sockets, credentials, raw `invoke`, or Runner mechanics.

### Useful later capabilities that are not parity blockers

The following omissions matter for a broader computer-use product, but they do
not block matching AIRI's current native tool set:

- drag/drop and explicit mouse-down/mouse-up;
- a public current-pointer-position read;
- public window move/resize/minimize/restore/zoom (platform Driver methods
  exist, but AIRI does not currently expose matching model tools);
- general AX press/set-value/action dispatch;
- a semantic post-action verifier that proves the requested UI state rather
  than only input delivery;
- compressed or artifact-referenced capture results for repeated large-frame
  model interactions.

AUV already makes the important distinction that `InputActionResult` proves a
delivery attempt, not semantic success. AIRI should preserve that boundary and
add verification as a separate observation when a workflow requires it.

## Combined assessment

For a macOS-first AIRI integration, the distance is **medium and bounded**, not
a rewrite:

- The sidecar/runtime topology is already implemented in both repositories.
- One signed packaged proof is needed to settle nested signing and TCC UX.
- The visual action loop needs the large-frame transport fix and scroll before
  it is comfortable for normal desktop use.
- Near-parity with AIRI's current native lane is roughly six capability slices
  plus one AIRI grounding adapter, as listed above.
- AIRI should replace the lower executor first. Deleting the entire
  `computer-use-mcp` process is a later migration of AIRI-owned policy,
  approval, grounding, state, and overlay consumers, not something the AUV
  binary alone can accomplish.

macOS is close enough for an end-to-end proof now. Linux remains conditional on
distribution and portal evidence. Windows is not on the same timeline until
AUV gains non-Unix local Runner IPC.
