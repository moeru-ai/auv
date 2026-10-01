# macOS Device Entry Host

This package implements the graphical helper for unlocking an existing macOS
console session after its user locks the screen. One signed, installed helper
passed a supervised Device API gate on the spare Mac. That result does not
establish general macOS release support or signed-out login.

The `AUV Device Entry Host.app` bundle is signed with a stable Apple-issued
identity from pinned Team ID `433DLLA855`, then installed root-owned at
`/Library/Application Support/AUV/AUV Device Entry Host.app`. Its Aqua
LaunchAgent runs as the already logged-in user. Accessibility and Post Event
permission must be granted to the **installed** bundle identity. A root-owned
daemon sends only a selected session UID/UUID and unlock intent over the
helper's private Unix socket. Credential bytes cross that socket only once
during target-local enrollment. The helper stores them in that UID's explicit
`~/Library/Keychains/login.keychain-db` item with service
`dev.auv.device-entry.v1` and account `uid:<uid>`; only the helper reads the
item for unlock. The wire response is one status byte, never secret data.

The client checks the accepted socket peer's process signature, root-owned
installed path, and root-owned Team ID pin before writing an enrollment
credential. The helper checks the
kernel peer UID (root or its own UID) and rejects every request for another
UID. It rechecks the same physical console session and lock state before
Keychain read and native input, then independently observes that same session
becoming usable. Keychain read during lock must succeed without a prompt.

`package/package.sh` builds and signs a reviewable app bundle when
`AUV_MACOS_SIGN_IDENTITY` is set. `package/install.sh` installs the reviewed
bundle and LaunchAgent; neither script is run by the build. The installed
path and code-signing requirement are part of the IPC identity contract.

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
