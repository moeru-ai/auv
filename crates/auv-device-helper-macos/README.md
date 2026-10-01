# AUV Helper for macOS

This package implements the graphical helper for unlocking an existing macOS
console session after its user locks the screen. One signed, installed helper
under the former `dev.moeru.auv.device-entry-host` identity passed a supervised
Device API gate on the spare Mac. The `ai.moeru.auv.helper` identity introduced
on 2026-10-01 still requires the same installed-host gate. Neither result
establishes general macOS release support or signed-out login.

The `AUV Helper.app` bundle is signed with a stable Apple-issued identity from
pinned Team ID `433DLLA855`, then installed for the current user at
`~/Library/Application Support/AUV/AUV Helper.app`. Its embedded LaunchAgent is
registered through `SMAppService.agent` and runs in that user's Aqua session.
This setup interface requires macOS 13 or later.
Accessibility and Post Event permission must be granted to the **installed**
bundle identity. A root-owned daemon sends only a selected session UID/UUID and
unlock intent over the helper's private Unix socket. Credential bytes cross
that socket only once during target-local enrollment. The helper stores them in
that UID's explicit
`~/Library/Keychains/login.keychain-db` item with service
`ai.moeru.auv.device-entry.v1` and account `uid:<uid>`; only the helper reads the
item for unlock. The wire response is one status byte, never secret data.

The client checks the accepted socket peer's process signature, exact per-user
installed path, pinned bundle identifier, and Team ID before writing an
enrollment credential. The helper checks the
kernel peer UID (root or its own UID) and rejects every request for another
UID. It rechecks the same physical console session and lock state before
Keychain read and native input, then independently observes that same session
becoming usable. Keychain read during lock must succeed without a prompt.

`package/package.sh` builds and signs a reviewable app bundle when
`AUV_MACOS_SIGN_IDENTITY` is set. It compiles the checked-in Icon Composer
source into both a Liquid Glass `Assets.car` and a fallback `.icns`. The single
`AUV Helper.icon` document contains Default and Dark appearance overrides, so
supported macOS versions select the matching appearance automatically. The
script also replaces the Info.plist version placeholder from the helper's Cargo
package version before signing.

Official macOS release builds embed an archive containing the signed and
notarized app. Users manage it through the shared setup interface:

```sh
auv setup macos-helper install
auv setup macos-helper uninstall
auv setup macos-helper status
auv setup macos-helper open-background-items-settings
auv setup macos-helper open-accessibility-settings
```

The install command extracts the app under a private directory inside AUV's
user Application Support root, validates its version, Team ID, signature, and
Gatekeeper assessment, then atomically places it at the stable path. The
installed helper registers its embedded LaunchAgent through ServiceManagement;
the setup module reads that native status and waits for the private socket.
This flow needs neither `sudo` nor an administrator password. A user who has
disabled the background item must enable it in Login Items & Extensions.
Accessibility remains separate TCC authorization for the installed bundle.
Uninstall waits for ServiceManagement to terminate the helper, resets only
that bundle's Accessibility decision, and removes only `AUV Helper.app`.
Enrollment remains in the login Keychain, and other AUV Application Support
content is preserved. An installed app that fails validation is removed
without being executed, so `uninstall` also recovers an `invalid` install.

Source builds do not contain Apple release credentials or an embedded app.
They report `helper_embedded=false`; status and registration remain available
for a valid installed helper at the same version. A fresh install or required
version update fails with an actionable payload-unavailable error. Release CI
supplies `AUV_MACOS_HELPER_APP_ARCHIVE_PATH` only after building, signing,
notarizing, and stapling the app.

`package/package.sh` is release build tooling, not the end-user installation
interface. It builds the signed app consumed by the release pipeline.

## Cargo features

- `setup` owns per-user installation, identity validation, and readiness
  inspection. It does not compile the macOS input driver or Swift package.
- `transport` owns daemon-side requests over the private socket and depends on
  the typed macOS driver result used by that protocol.
- `host` includes `transport` plus the graphical helper server,
  ServiceManagement bridge, Keychain vault, and native input delivery. It is
  the default used to build `AUV Helper.app`.

## Host validation

The target-local enrollment service marks an account `PENDING` after storage.
While the exact selected session is locked, `probe_locked` reads the item
without prompting before the account advances to `READY`. The spare-Mac gate
confirmed retrieval and two supervised black-display unlocks through the
actual Device API, with same-session readback and owner observation. See the
[macOS locked-session host
gate](../../docs/ai/references/session-api/2026-09-28-macos-locked-session-host-gate.md).
Other host configurations and release installation remain open.

NOTICE: The login Keychain choice is limited to the locked-existing-session
release. Availability after logout is unproved and is outside this package's
scope. A generic Keychain item created by this helper may still be subject to
macOS access control prompts; the locked retrieval gate determines whether
this exact installed signature and item ACL work on the target host.
