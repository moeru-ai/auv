# macOS locked-session Device host: package and live gate

Status: configuration-specific installed-host and local Unix DeviceService
proof on macOS 26.3, recorded on 2026-09-29. Two supervised black-display
DeviceService unlocks passed with same-session OS readback and owner-visible
desktop confirmation. The client used an owner-verified local root Unix
socket; paired authentication over a network endpoint was not exercised.
System sleep, closed-lid wake, signed-out login, and other configurations
remain unproved.

## Record provenance

Extracted from [PR #198](https://github.com/moeru-ai/auv/pull/198) at
[`85f6385`](https://github.com/moeru-ai/auv/tree/85f638525c669e56aac3ce4792e39292563944c6).
This is a research and experiment record; the candidate implementation and
policy decisions remain in that separate PR. Code, API, packaging, and
installation descriptions below refer to that candidate or to the dated
experimental stage, not to functionality added by this document.

The extraction revision is not the exact build used in every live gate.
Keep the recorded source/binary hashes and stage labels as the experiment
identifiers; where a gate gives no exact revision, that provenance remains
unrecorded. No experiments were rerun for this extraction. Later successful
gates supersede only the earlier limits they explicitly retested.

## Experiment chronology

The original installed-host and local Unix DeviceService record began on
2026-09-29 for one existing, logged-in locked physical console session on
`neko-mbp-m1` (macOS 26.3).
The helper has compiled, passed narrow tests, and produced a signed review
bundle. Review6 was
installed on `neko-mbp-m1`; its installed Mach-O hash, Developer ID signature,
root ownership, Aqua LaunchAgent process, and private socket were checked.
Target-local enrollment through the root daemon and signed Aqua helper returned
`PENDING` after the helper's immediate Keychain readback. A supervised,
read-only probe then observed that exact `neko` session locked, called the
production `probe_locked` client, and received success from the installed
helper while the same session remained locked. The probe emitted no input or
credential bytes. It called the helper client directly, so it did not promote
the daemon's `PENDING` metadata. That first test established locked Keychain
retrieval. The review9 delivery result and later DeviceService gate appear below.

The daemon now has a private macOS `DeviceLocalControl` backend in
`device_entry/enrollment_macos.rs` and an `UnlockHost` adapter in
`device_entry/host_macos.rs`. The local service uses the same metadata,
account locks, and audit instances as the future remote policy. It resolves
the requested name through the OS account database to a stable UID and home,
permits enrollment only from that UID or root, and reserves policy mutation
for root. It accepts only an OS password in protected storage. A successful
Keychain write publishes `PENDING`; the remote DeviceService was disabled at
the time of the review6 gate.
`probe_locked` is the only path that can later promote it to `READY` while
the exact selected session is locked. The daemon has no vault read endpoint.
The macOS DeviceService policy was subsequently connected to these same
metadata, lock, and audit instances.
Focused daemon tests passed: three local enrollment tests and two selected
session/helper error mapping tests. These tests do not exercise an installed
signed helper or a locked Mac.

The earlier installed review6 build was
`target/device-entry-macos-review6-20260928/AUV Device Entry Host.app`.
Its Mach-O SHA-256 was
`4741789eca233f493372b8b2ec65fee4edf6c406aaeb4d8ece1bfe74e3c80c20`.
`codesign --verify --strict` passed the bundle identifier and Team ID
`433DLLA855` requirement for Developer ID Application `MOERU AI LTD`.
Three focused helper tests passed, including same-UID denial for all four
operations. This ignored local artifact is not an installed-path result.

The review9 candidate, now installed for the supervised gate, is
`target/device-entry-macos-review9-20260929/AUV Device Entry Host.app`,
Mach-O SHA-256
`ba4b2c79421e1a1df6db7bb59d77a5c1cd01c2fe32820888d258f4e476da944c`.
Its Developer ID signature and Team ID were verified locally, after staging,
and at the root-owned installation path. It replaced the cleaned review6
installation. Four focused helper tests
passed after the review fixes to path derivation and typed operation decoding.

The review9 root daemon ran only on a private Unix listener. Target-local
`policy get` reported `true`, and target-local hidden-input enrollment for
`neko` returned `uid:501 pending protected`. The installed Aqua LaunchAgent
ran as UID 501 with a `0600` private socket; the owner enabled Accessibility
for that installed app. A one-shot, root-owned gate executable (SHA-256
`ca3bca4a7427925e3b060a71fad1dd7f75202477e30d66ec91d20ecc32be24fe`)
observed the original unlocked session UUID, waited for its lock, called the
production signed-helper `probe_locked`, then made exactly one production
signed-helper `unlock` call. Its non-secret log reported
`locked_preflight=ok`, `locked_keychain_retrieval=ok`,
`same_session_still_locked=true`, `unlock_attempt_started=true`, and
`same_session_usable=true`. A separate IORegistry read showed
`IOConsoleLocked=No` and the unchanged UID 501/session UUID; the owner reported
that the Mac automatically returned to `neko`'s desktop. No credential was
logged or sent through the paired API. This is installed-helper behavior on
this tested host; a paired DeviceService end-to-end gate and other macOS
versions/session arrangements remain unproven.

The owner repeated the normal visible-lock route several times and reported
automatic unlock without a password error. A separate attempt made while the
display was off did **not** unlock: the one-shot log reached
`locked_keychain_retrieval=ok` and `unlock_attempt_started=true`, then returned
the harness's coarse `installed_helper_unlock_failed`. `pmset -g log` recorded
display-off at 03:02:34 and display-on at 03:02:48; the gate log was written
at 03:02:35. The harness did not preserve whether the helper returned
`InputUnavailable` or `OutcomeUnverified`, so the exact input failure is not
yet established. A later non-secret check used `pmset displaysleepnow` while
the same session was locked, then `caffeinate -u -t 5`: power logs recorded
display-off and display-on, IORegistry still reported locked, and the owner
saw the `neko` lock screen still requiring a password. This proves a possible
display-wake step, not black-display unlock. At that point the installed helper
had not yet been changed or retested for that state.

Review10 adds an input-before-wake gate using the SDK's
`IOPMAssertionDeclareUserActivity` with `kIOPMUserActiveRemote`. It waits for
`CGDisplayIsAsleep(CGMainDisplayID())` to become false within the existing
posting deadline, rechecks the selected locked console, then follows the
previous activation/clear/credential route. Its installed Mach-O SHA-256 is
`9c83b89cd837bfb649d07c240ef2026d972404b193be6bb79a622f564d682db7`;
the root-owned app's Developer ID signature, running Aqua LaunchAgent, and
private socket were verified after replacing review9. The review9 bundle
remains as a root-owned rollback copy. A separate one-shot black-display gate
(SHA-256 `d0cf8197a103463f57c1338fa59078a3f69537c7bf67be888ed29138505c0746`)
called `pmset displaysleepnow` after the same session locked, confirmed
CoreGraphics reported the display asleep after three seconds and again just
before the sole unlock call, then reported locked Keychain retrieval and
`same_session_usable=true`. Power logs recorded display-off at 03:21:17 and
display-on at 03:21:20; independent IORegistry readback reported unlocked.
The owner confirmed that the Mac automatically returned to the desktop. This
proves the tested display-sleep state on the spare Mac; system sleep and
closed-lid wake are outside this gate.

The subsequent DeviceService gate used a rebuilt root daemon on the same
private Unix socket, with the review10 signed Aqua helper and the existing
`PENDING` enrollment. `devices sessions --json` returned exactly one
`USABLE` physical console session for `neko`, selector
`macos:4EE0334E-8E89-4EDB-A771-6B46FEC37B92`. A root-owned, one-shot
gate (SHA-256
`4649c21b18f2693144d43ba5738e8c07827f5885985de24fd3ba0371d01a24ac`)
waited for that session to lock, requested display sleep, checked the main
display was asleep after three seconds and immediately before calling
`devices unlock --session <same selector>` through DeviceService. It required
the returned effect to be `UNLOCKED_EXISTING_SESSION` for that exact selector
and user, then independently observed the same session usable. `pmset -g log`
recorded display-off at 03:30:09 and display-on at 03:30:12. The owner saw
the Mac automatically return to the desktop. A target-local read returned
`neko uid:501 ready protected`, confirming `PENDING` promotion. The gate
used a local root Unix client. Paired authentication over a network endpoint,
system sleep, closed-lid wake, and other users or Mac versions remain
unproven.

The local audit contained an `attempt` and matching `outcome` with
`UNLOCKED_EXISTING_SESSION` for that selector, without credential material.
A second black-display DeviceService call, now with `READY` enrollment,
returned the same effect and independent same-session usable readback; the
owner again observed an automatic return to the desktop. The probe enrollment
was removed after these gates, and a separate Keychain metadata lookup found
the exact `dev.auv.device-entry.v1`/`uid:501` item absent.
The foreground daemon was stopped and the reviewed cleanup removed the
installed review10 app, review9 rollback copy, LaunchAgent, Team ID pin,
private gate directory, staging directories, and user socket. Independent
path, process, LaunchAgent, and Keychain-item checks found them absent.

On `neko-mbp-m1` (macOS 26.3, arm64), the review6 root daemon returned
`remote unlock enabled: true` through its private Device-local socket.
`Enroll --user neko --kind os-password` returned `neko uid:501 pending
protected`. The separate one-shot read-only probe binary had SHA-256
`9d6da0af704b5f2604b362e9ee7bac973f8b7e151221b0d29a86e9654acaea86`.
While the owner locked the existing session, it recorded
`locked_preflight=ok`, `same_session_still_locked=true`, and
`locked_keychain_retrieval=ok`. The probe used the production client and
installed signed helper, but did not call DeviceService, post input, or prove
an unlock.
After the probe, `RemoveEnrollment` returned `enrollment removed` and an
independent `security find-generic-password` lookup returned item-not-found.
The foreground daemon was stopped; the helper LaunchAgent was booted out;
the unique root gate directory, installed app, LaunchAgent plist, Team ID pin,
stale user socket, and user staging directory were removed. A final process
and path check found no gate process or installed artifact.

## Package and authority

`crates/auv-device-helper-macos` builds a Rust executable using
`auv-driver-macos::device_session_unlock::submit`. `package/package.sh` builds
an app bundle with identifier `dev.moeru.auv.device-entry-host` and requires
`AUV_MACOS_SIGN_IDENTITY` to name the reviewed Developer ID Application
certificate. The first package pins Team ID `433DLLA855` in code and both
packaging scripts; another team requires an explicit code review.
It refuses ad-hoc signing. `package/install.sh` verifies the signature and
identifier and installs the app root-owned at
`/Library/Application Support/AUV/AUV Device Entry Host.app`, plus the
root-owned `/Library/LaunchAgents/dev.moeru.auv.device-entry-host.plist` and
`/Library/Application Support/AUV/device-entry-host.team-id`. The installer
verifies that the signed package matches the reviewed Team ID.
The LaunchAgent runs only in an already logged-in user's Aqua context. The
installed app identity must receive Accessibility and Post Event permission.

Each user helper creates `~/Library/Application Support/AUV/device-entry` with
mode `0700` and `host.sock` with mode `0600`. The client obtains the accepted
socket's kernel peer **audit token** through
[`LOCAL_PEERTOKEN`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/un.h), then asks
[Security.framework](https://developer.apple.com/documentation/security/seccodecopyguestwithattributes%28_%3A_%3A_%3A_%3A%29)
for that specific guest code using
[`kSecGuestAttributeAudit`](https://developer.apple.com/documentation/security/guest-attribute-dictionary-keys).
It checks the running code against the bundle identifier,
Apple signing anchor, and root-owned pinned Team ID, and requires its code path
to be the root-owned installed app before transmitting any enrollment credential.
It checks the ownership and permissions of every installed path ancestor. The
helper requires a **root peer for every operation**. A same-UID process could
otherwise replace or remove a Keychain item without updating the daemon's
metadata generation, or bypass the daemon's switch and audit when probing or
unlocking. Every request's target UID must equal the helper's own UID.
The macOS Device daemon therefore must run as root for remote unlock in this
candidate. The current short Device-local socket directory is root-only in
that deployment, so target-local enrollment uses a root-run CLI; ordinary
same-UID self-enrollment requires a separately reviewed reachable local IPC
path. These checks do not replace `DeviceLocalService` account authorization
or paired DeviceService policy.

The helper alone reads/writes the explicit per-user
`~/Library/Keychains/login.keychain-db` item keyed by service
`dev.auv.device-entry.v1`, account `uid:<uid>`. Enrollment bytes are sent only
over this local authenticated socket; unlock requests carry only UID and
`macos:<session UUID>`. The helper responds with a one-byte, non-secret status.
It disables Keychain UI during retrieval, rechecks the selected locked console
session before native delivery, and observes the same session afterward.
Both daemon and helper call `auv-driver-macos::device_session::observe_console`
for separate live reads with one UID, account-name, UUID, and lock parser.
`probe_locked` reads the item while that selected session is locked without
posting input. No probe or posting success alone is an unlock claim.
The helper accepts only an OS login password for macOS; `DeviceLocalService`
must reject Windows PIN and other credential kinds before forwarding the
enrollment bytes to this protocol.

## Reviewable staging and gate

1. Review the helper source, package scripts, installed paths, signing
   certificate identity, and Team ID. Build the app on the target with
   `AUV_MACOS_SIGN_IDENTITY='Developer ID Application: MOERU AI LTD (433DLLA855)' package/package.sh <empty-output-dir>`.
   Verify the app's code signature, designated requirement, and bundle ID.
2. With `neko` already logged in, install the reviewed package through
   `sudo package/install.sh '<app-path>'`. Load the Aqua job in the current
   graphical login domain, or log out and back in, then confirm the running
   helper has the installed path/signature, UID `neko`, private socket mode,
   and Accessibility/Post Event grants. Installation itself must not send
   input or read a credential.
3. Run the target-local hidden-input `DeviceLocalService.Enroll` CLI as root
   with the daemon's absolute `--store-root` path to store one OS password.
   Its enrollment remains `PENDING` after immediate readback.
   The service must resolve `neko` to the stable UID and authorize the actual
   local peer UID before it invokes the helper. No credential appears in argv,
   environment, logs, audit, Run, trace, or a remote RPC.
4. Under owner supervision, lock the existing `neko` console session. Confirm
   IORegistry still identifies the exact session UUID and reports locked.
   Invoke `probe_locked` once; it must return success from the installed
   helper without a Keychain prompt, while the session remains locked. Only
   then may enrollment advance from `PENDING` to `READY`.
5. Invoke one unlock through the actual paired DeviceService path. The helper
   must recheck UID, UUID, and locked state, retrieve locally, and post once.
   Both helper and daemon must independently read the **same** console UUID as
   usable, and the owner must see the desktop return. A posting call, timed
   wait, or permission boolean is insufficient. On failure, stop; do not
   automatically retry a credential or mark it rejected without OS evidence.
   Repeat a non-secret input-state check with the password field already
   focused and with manually entered harmless partial input before enabling
   unattended use: the current native primitive posts Return to focus and
   could submit retained input. A clear/focus strategy must be validated
   without creating extra failed login attempts.

The proposed input route uses one harmless, non-submitting character to reveal
the secure field, requires that field to be uniquely identified and focused,
then posts Command+A and Backspace before the credential. It must recheck the
same locked session and secure-field focus after clearing. macOS does not
expose a character count through this field's AX attributes, so clearing is
supported by the controlled visible-input gates below, not by an AX emptiness
readback. The installed helper and actual DeviceService still require a
supervised credential-delivery gate and independent post-action verification
before this route can be enabled. The earlier Return-to-focus primitive was
removed because it could submit retained partial input.

A follow-up signed, read-only AX diagnostic on the same locked `neko` session
tested this visibility question without a credential or posted event. Its
Mach-O SHA-256 was
`feca06322ade25c035f21fd7dfea8822788f820209022f9860fbf4cbfd452eb9`.
Before the owner clicked the password field, the `loginwindow` AX tree showed
no secure field. After a physical click, the diagnostic found one focused,
enabled `AXTextField` with `AXSecureTextField` subrole. A request for
[`AXNumberOfCharacters`](https://developer.apple.com/documentation/applicationservices/kaxnumberofcharactersattribute)
returned `-25205` (`kAXErrorAttributeUnsupported` in the macOS SDK),
so this API did **not** establish whether the field was empty. The exact
console session stayed locked. This supports field identification after a
human click, but leaves the unattended focus and clear-state gates open.
The owner then unlocked manually. The one-shot diagnostic had exited, and its
private log and staged app were removed; IORegistry showed the same user at
the console with `IOConsoleLocked=false`. The manual Accessibility grant for
that throwaway app may remain in System Settings until the owner removes it.

On 2026-09-29, a separate signed, non-secret one-shot probe tested selection
and clearing on the same Mac. From the initial lock screen, a posted Tab did
not reveal a unique focused secure field; the probe stopped before posting
Command+A, Backspace, or a digit. The owner then physically selected the
password field, entered one harmless `1`, and confirmed one dot. A fresh probe
identified the unique focused secure field, posted Command+A and Backspace,
and paused. The owner observed zero dots and no password error. This is
evidence that the selection and clearing sequence worked on an already
visible, focused field with one partial character. It does not establish a
target-side empty-state signal or unattended field activation. After the
pause, the field or session no longer matched the probe's identity check, so
the probe stopped before posting its planned harmless `2`. No credential or
Return was sent.

A second signed one-shot probe then tested activation and clearing from an
untouched lock screen. Its Mach-O SHA-256 was
`d3812cddc94fcf7140c52515f304d17947894885f6d22820f95891f74333a9b7`.
It posted a harmless Unicode `2`, found one focused secure field in the same
locked session, posted Command+A and Backspace, immediately rechecked that
field and session, then posted a harmless Unicode `3`. It sent no Return and
read no field value or credential. The owner observed exactly one resulting
password dot and no error; IORegistry still reported the same session locked.
This is a visible-input gate for that sequence on this host. It does not prove
that the installed helper can deliver a real credential or that DeviceService
can verify the unlock outcome.

The native candidate now replaces its preliminary Return with that tested
activation and clearing order. It passes the selected `macos:<UUID>` into
Swift, checks the same locked physical console and exact focused loginwindow
field before each credential character and before its single final Return,
and stops posting if its seven-second budget expires. The helper reserves ten
seconds for independent same-session readback under its request timeout.
Compilation and focused tests are implementation evidence only. One read-only
lookup probe timed out while the target remained unlocked. A second signed,
read-only probe launched in the existing locked session and found exactly one
`com.apple.loginwindow` process (PID 51703) through the production
`NSRunningApplication` lookup. It reported no initially focused secure field,
as expected on the untouched lock screen. It then timed out waiting for the
trigger after owner confirmation of a physical click, so AX traversal after
the field appears remains unverified. Neither probe sent input or read a
credential.
At that point the production route remained disabled pending this check and
supervised installed credential delivery with OS state and owner-visible
readback.

A follow-up read-only five-minute poll in the same locked session continued
to see no unique focused secure field and timed out. The owner did not confirm
clicking the field during that poll, so the timeout is not evidence that the
AX traversal fails after a click. Its signed temporary app and private log
were removed; the Mac remained locked. No input was posted.

After the owner physically selected the password field, a signed read-only
diagnostic isolated why the stricter production traversal still returned nil:
the single enabled, focused `AXSecureTextField` was present under loginwindow
PID 51703, but a sibling `AXButton` returned `-25212` for `AXChildren`.
The local macOS SDK defines `-25212` as `kAXErrorNoValue`. The native
`axChildren` helper now treats that result as an empty leaf, while other AX
read failures still abort. A second signed read-only diagnostic, with this
one change, traversed all 13 observed elements, found exactly one enabled
secure field equal to loginwindow's focused element, and reported
`production_nil=none` in the same locked session. It sent no input and read no
field text. The diagnostic app and logs were removed from the target. This
proves the corrected **read-only AX selection** on this host, not real
credential delivery or unlock.

The audit-token lookup avoids selecting another process after numeric PID
reuse. If the connected process exits before dynamic validation, guest lookup
must fail closed; the installed gate should exercise that case. The gate is for
the tested macOS version and physical-console arrangement. A
separate signed-out login test and a separate multi-session host selection
design would be needed for those states.

## Rollback

While logged in, first call target-local `RemoveEnrollment` so the helper
deletes its exact Keychain item and verifies it is absent. Boot out the
`dev.moeru.auv.device-entry-host` job from the user's Aqua domain. Remove only
its unique LaunchAgent plist, Team ID pin, and app bundle, then confirm the process and
socket are absent. The owner can remove the app's Accessibility grant in
System Settings. If the service is unavailable before deletion, remove the
exact service/account item through Keychain Access under the affected user;
do not send credential material through a terminal or remote request. Preserve
the daemon's disabled path until a fresh installed gate passes.
