# macOS Helper Setup

Status: per-user `SMAppService` implementation complete on 2026-10-01;
configuration-specific signed and notarized installed gate passed on 2026-10-02;
shipped helper identities added on 2026-10-03.
Clean-host release installation and broader configurations remain release
gates.

## Contract

The official macOS CLI and `@auv-js/cli` native binding use one Rust setup
module owned by `auv-device-helper-macos`. Its public interface reports status,
installs or uninstalls the signed app for the current user, registers its
embedded LaunchAgent, and opens the Background Items and Accessibility settings panes.
CLI and JavaScript are adapters and do not duplicate installation policy.

An official release build embeds an immutable ZIP only after the release
pipeline has:

1. built the architecture-matched `AUV Helper.app` with its LaunchAgent plist
   under `Contents/Library/LaunchAgents`;
2. signed the complete app with the pinned Developer ID Application identity;
3. notarized the app archive and stapled the ticket to the app;
4. reassessed the stapled app with Gatekeeper;
5. archived the final app for binary embedding.

`package/package.sh` derives `CFBundleShortVersionString` from the helper Cargo
package before signing; the checked-in Info.plist contains only a placeholder.
The release bump configuration separately updates the independent unpublished
N-API Cargo manifest so its compiled version stays aligned with the npm package.

Source builds intentionally embed no app archive. Status and registration of
any valid, protocol-compatible installed helper remain available. A fresh
install or a required protocol update returns a payload-unavailable error. This keeps Apple
release credentials out of ordinary Cargo builds and prevents local ad-hoc
signing from becoming a TCC identity.

## User interaction

`auv setup macos-helper install` writes the embedded archive to a mode-`0600`
file in a private staging directory under
`~/Library/Application Support/AUV`, extracts it, and validates the app version,
bundle identifier, pinned Team ID, and static signature. Setup does not run a
Gatekeeper assessment; macOS enforces notarization on downloaded apps, and the
release workflows assess their artifacts before publishing.
It then atomically places the app at
`~/Library/Application Support/AUV/AUV Helper.app`.

The installed helper calls `SMAppService.agent` for the plist inside its own
bundle. Registration does not use `sudo`, an administrator credential,
`/Library/LaunchAgents`, or direct `launchctl` calls. A user who disabled the
background item receives `requires-approval`; setup can open System Settings >
General > Login Items & Extensions but cannot approve it.

Accessibility is a separate TCC decision for the installed helper identity.
Setup can open that System Settings pane but cannot grant the permission. The
helper remains limited to locked-existing-session operations; this slice does
not route ordinary AUV input or AX inspection through it.

`auv setup macos-helper uninstall` first validates the installed app, then asks
`SMAppService` to unregister the LaunchAgent and waits for its completion
handler. Apple invokes that completion only after the running helper has been
terminated, so an in-flight request is canceled at the process boundary before
files are removed. Setup then resets the `Accessibility` TCC decision for only
`ai.moeru.auv.helper` and removes only `AUV Helper.app`. The enrollment remains
in the user's login Keychain, and the surrounding AUV Application Support
directory, private socket directory, and other contents remain in place. A
failure in unregistration, TCC reset, or app removal is reported; uninstall
does not delete broader state or attempt a rollback.

An installed app that fails static validation is never executed. `install`
refuses it and points to `uninstall`; `uninstall` skips the
ServiceManagement call, still resets TCC and removes the bundle, and reports in
`detail` that the LaunchAgent registration may remain. A later install places
the new app at the same path, so that registration launches the validated
replacement.

Whether an installed helper is usable is decided by wire protocol
compatibility, not by matching the frontend's version; see
[helper protocol compatibility](2026-10-02-macos-helper-protocol-compatibility.md).

The per-user installation weakens filesystem ownership relative to the former
root-owned experiment. Runtime verification rejects unsigned changes and code
from another bundle identifier or Team ID. Older signed helpers are revoked by
raising the signed security epoch (`AUVHelperSecurityEpoch`) together with
the daemon's `MIN_SECURITY_EPOCH`; see
[helper compatibility and revocation](2026-10-02-macos-helper-protocol-compatibility.md#security-epoch-revocation).

## Shipped helper identity

An application that embeds AUV, such as an Electron app bundling the `auv`
executable, may ship the helper under its own name, icon, bundle identifier,
and Developer ID team. Setup and the daemon do not pin the official identity
in that case:

- Every frontend takes the unpacked, signed helper app as a typed option:
  `setup::Options::helper_app` in Rust, `--helper-app` (or
  `AUV_MACOS_HELPER_APP`) for `auv setup macos-helper`, `helperApp` for the
  `@auv-js/cli` setup functions, and `startAuv({ platforms: { macos: {
  helperApp } } })` in the SDK. The SDK passes it to the daemon as
  `AUV_MACOS_HELPER_APP`, the daemon's process-boundary contract, like
  `AUV_OVERLAY_THEME` for overlay themes. Without it, the official
  `ai.moeru.auv.helper` / `433DLLA855` identity and the embedded archive are
  used exactly as before.
- When it is given, the app must be validly signed by an Apple-issued
  certificate with a Team ID. Its signing identifier and Team ID become the
  trusted identity for this process; a missing or invalid app makes `status`
  report `invalid` and `install` fail with the reason.
- The app installs as a copy at
  `~/Library/Application Support/<bundle identifier>/<App Name>.app`, never in
  place, because the containing application may be replaced by its updater
  while TCC and ServiceManagement need a stable path. Its socket is
  `.../<bundle identifier>/device-entry/host.sock`, and the helper derives
  that directory from its own installed location. Its Keychain service is
  `<bundle identifier>.device-entry.v1`, because a Keychain item's ACL trusts
  only the helper that created it. Each identity therefore coexists with the
  official helper.
- `helper_embedded` reports whether an install payload is available from
  either source. Upgrades compare the installed version with the shipped app's
  `CFBundleShortVersionString`; the protocol and security epoch rules are
  unchanged.
- `package/package.sh` builds such an app with `AUV_MACOS_HELPER_NAME`,
  `AUV_MACOS_HELPER_BUNDLE_ID`, `AUV_MACOS_HELPER_ICON` (`.icon` or `.icns`),
  and `AUV_MACOS_TEAM_ID`. The LaunchAgent plist and label follow the bundle
  identifier, and the helper registers `<bundle identifier>.plist`.

Trusting the identity chosen by the process that launches AUV does not weaken
the boundary: whoever controls that environment already controls the daemon
that sends the enrollment credential. The peer of the private socket must
still match one bundle identifier and Team ID at the expected per-user path.

## Frontends

- CLI:
  `auv setup macos-helper status|install|uninstall|open-background-items-settings|open-accessibility-settings`.
- Node/Electron: asynchronous `macosHelperStatus()`,
  `installMacosHelper()`, and `uninstallMacosHelper()`,
  `openMacosHelperBackgroundItemsSettings()`, and
  `openMacosHelperAccessibilitySettings()` from `@auv-js/cli`.

Both status surfaces return the same state, optional validation detail, and
whether the current executable or binding embeds a release helper app.
Node operations that inspect, install, or uninstall the helper return promises;
their filesystem, signature, subprocess, and ServiceManagement work runs
outside the JavaScript thread.
If the serial helper is busy and its queued socket has no peer audit token yet,
status reports `busy`, and install leaves the valid running registration alone.
Only a resolved peer that fails the pinned identity or installed-path checks is
`invalid`.
After a new registration, `busy` is not sufficient readiness: setup waits for
`running` or returns an activation failure so an update can restore the prior
app. Rollback first unregisters the replacement, so its process stops before
its bundle is removed; a failed fresh install leaves no registration behind.
If the replacement cannot be stopped, it stays in place and the previous app is
retained in the staging directory. Readiness polling repeats only the
ServiceManagement and socket checks; the static file, signature, and version
checks run once before registration. `requires-approval` remains a successful registration that needs explicit
user action in System Settings.

The N-API crate enables only the helper crate's `setup` feature. The
daemon-side socket transport and graphical host are separate features, so the
Node binding does not compile `auv-driver-macos`, its Swift package, or the
Tokio host. Because `js/packages/cli` is an independent Cargo workspace, its
Cargo.lock is committed and release N-API builds pass Cargo `--locked`.

## Validation evidence

The source build compiles and exercises status through the shared CLI and N-API
interfaces. Unit tests cover the terminal ServiceManagement unregistration
states, readiness classification, uninstall scope, and restoration of the
previous app when a replacement does not become ready.

On 2026-10-02, an arm64 macOS 26.3 host (`neko-mbp-m1`) installed an embedded
0.0.22 app signed by Team ID `433DLLA855`, notarized in submission
`d68f8fdb-64b4-4f56-953c-0f5ea02fd6e8`, and accepted by Gatekeeper. The
per-user `SMAppService` registration became enabled and running, enrollment
under `ai.moeru.auv.device-entry.v1` succeeded, and a paired Device completed:

- one open-lid `lock → LOCKED → unlock → USABLE` sequence; and
- two `LOCKED → close lid → unlock → USABLE` sequences, with the owner
  observing the built-in display wake after unlock.

On 2026-10-03, the same host packaged a renamed helper
(`AUV Helper Identity Test`, bundle identifier
`ai.moeru.auv.helper.identity-test`, `.icns` icon) with the package script
overrides, signed it with Team ID `433DLLA855`, notarized it in submission
`910b0a1e-71c7-4c3a-bacd-fd81c464befb`, and passed it through
`AUV_MACOS_HELPER_APP` to a source-built CLI; the same install and uninstall
were repeated with `--helper-app`, and the N-API `macosHelperStatus({ helperApp
})` reported the shipped and rejected apps. Install placed it under
`~/Library/Application Support/ai.moeru.auv.helper.identity-test/`, its
LaunchAgent registered and ran, status reported `running` through the peer
identity check on its own socket, a repeated install was a no-op, and uninstall
unregistered it and removed the app. The official helper's status was
unaffected. Device enrollment and unlock through a shipped helper were not
exercised.

This is configuration-specific installed behavior, not a general support or
release claim. A clean-host first install, denied Background Item recovery,
live update rollback, and live uninstall on a disposable installation remain
open release gates.
