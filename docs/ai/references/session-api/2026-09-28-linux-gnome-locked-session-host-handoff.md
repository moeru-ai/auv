# Linux GNOME locked-session Device host handoff

Status: configuration-specific paired DeviceService proof on 2026-09-29.
The final retest on one Debian 13 GNOME Wayland host passed two
Mac-originated paired unlocks of the same existing session, with target-local
PAM revalidation, independent logind readback, audit, and owner confirmation.
Both `PENDING` and `READY` enrollment were exercised. Automatic display wake,
rotated-password rejection, other desktop configurations, and release
installation remain unproved. See the [final retest](#2026-09-29-final-review-fix-retest).

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

## Initial candidate and subsequent revisions

The initial candidate kept the paired `DeviceService` route disabled with
`UNSUPPORTED_OS_STATE` while the same-UID host and credential rotation gates
were incomplete. The implementation descriptions and dated gates below
record that progression; the final retest supersedes those earlier disabled
route statements. They do not describe capabilities shipped by this docs PR.

`auv-driver-linux::device_unlock` inventories physical, active, local GNOME
Wayland sessions owned by its effective UID. An opaque selector includes the
logind session ID, numeric UID, and creation timestamp. Before `Unlock`, it
resolves the session again and compares ID, timestamp, UID, user, and seat,
then rereads Type, Class, Remote, and Active on the target logind object.
The initial candidate required agreement between logind `LockedHint` and
GNOME ScreenSaver `GetActive`. A supervised gate exposed that this conflates
two distinct GNOME states. The revised candidate uses `LockedHint` for the
selected session's lock state, including the final pre-delivery check and
same-session post-delivery readback. `Unlock` itself only requests that the
session manager remove the lock; target UI observation is still required in
the installed gate before claiming a verified effect.

The daemon's target-local `DeviceLocalService` uses a short, store-specific
Unix socket under a `0700` daemon-owned directory in the system temporary
root. The directory is atomically created, checked for owner and mode before
use, and removed after the socket stops, so
this candidate serves a same-UID daemon/user pair. Cross-UID root
administration needs an explicit target UID and server-identity gate. A
root-owned multi-account daemon cannot enroll another user's Secret
Service item through this path. The local backend resolves account names
through NSS, checks the kernel peer UID, and writes only `PENDING` metadata
after the installed `gdm-password` PAM service accepts exactly one hidden
password response for the unchanged account, its account policy succeeds, and
the user's default Secret Service collection accepts the credential. Missing
PAM, a passwordless or different-factor conversation, and PAM service errors
fail closed before either metadata or vault writes. Authentication rejections
return only a fixed, non-secret error. This is specific to the target's
configured GDM password policy, not a generic Linux password verifier.
The [Linux-PAM application API](https://github.com/linux-pam/linux-pam/blob/master/libpam/include/security/pam_appl.h)
and [conversation contract](https://man7.org/linux/man-pages/man3/pam_conv.3.html)
define the check; the tested Debian host's `gdm-password` service includes
`common-auth` with `pam_unix`.
It invalidates prior eligibility before a replacement and tombstones it
before deletion. It never invokes a Secret Service prompt automatically.

The private Linux host now implements the shared policy port. Its asynchronous
`PENDING` probe rechecks the exact locked session, retrieves and drops the
Secret Service item without output, then rechecks the session before policy
may mark it `READY`. The host rejects more than one eligible physical session
of its UID because this installed host gate has not validated multi-session
routing and effects, even with an explicit selector. This restriction may be
removed after a multi-session gate. The local store
accepts a same-UID OS password in the protected default collection and
publishes `PENDING`; logind's unlock method does not consume those bytes on
the tested host. The supervised test binary proved locked retrieval through
the production host and vault modules, while the paired daemon route remains
disabled.
Password rotation after enrollment cannot be inferred from Secret Service
readability or logind Unlock. A reviewed revalidation or revocation policy and
its tests are required before enabling paired Linux DeviceService.

## Installed gate

1. Install a same-UID daemon and local client for the logged-in `neko` session
   on `neko-gpu-1`. Confirm the daemon and session bus remain reachable when
   that existing session is locked. Use the `ihome.conf` Kubernetes route if
   SSH is unavailable, but preserve the actual host UID and session bus.
2. Enroll a target-local OS password through the hidden terminal CLI. Confirm
   only non-secret `PENDING` metadata is returned and that deletion/restart
   invalidates eligibility.
3. Under owner supervision, lock the existing GNOME session. Build the daemon
   test binary beforehand, then run only the ignored
   `device_entry::host_linux::tests::installed_locked_secret_retrieval_without_unlock`
   test under the daemon's exact UID with `AUV_LINUX_LOCKED_GATE_SELECTOR` set
   to the observed non-secret selector. The test uses the production host and
   vault code, emits no secret, sends no Unlock call, and requires the same
   session to remain locked afterward. Confirm the daemon's own session bus
   and lifecycle separately; a test binary is not an installed daemon process.
4. After the read-only gate, run the separate ignored
   `installed_locked_enrollment_unlock_once` test under the same UID and exact
   selector. It makes one logind `Unlock` request and checks same-session
   `LockedHint` readback. The owner must independently confirm that the GNOME
   UI returned to the desktop; a cleared hint alone is not that proof.
5. Route one paired Device request through policy and logind readback. Check
   ambiguity, stale selector, disabled switch,
   unenrolled account, and delivery without confirmed unlock. Verify audit
   records only allowlisted non-secret fields. Keep the paired route disabled
   until this gate passes.

The prior direct logind proof is recorded in
[the research note](2026-09-27-remote-device-unlock-research.md). A temporary
Rust Kubernetes pod on node `neko-gpu-1` compiled the Linux daemon candidate
with `cargo check -p auv-daemon --locked`; the seven focused Linux local
enrollment tests also passed there. Neither result is an installed locked-host
retrieval or paired Device unlock result.

On 2026-09-29, read-only target inspection found UID 1000 `neko` in active
physical Wayland session 52 on `seat0`, with `LockedHint=no`, a GNOME user
bus, and `org.freedesktop.secrets` reachable. Direct SSH and SSH through the
existing `rc-dev/lody-neko-gpu-1` pod using `ihome.conf` both authenticated
to the same host. No lock, enrollment, Secret Service retrieval, or unlock
was attempted in this inspection.

An isolated same-UID service and private Unix socket were subsequently staged
on `neko-gpu-1`. Native daemon Device entry tests passed (31 passed, one
ignored), but the service was stopped and its unique stage removed before
credential entry or lock testing. Review found that the former enrollment
accepted any nonempty stored string even though logind did not consume it;
the password verification change above is therefore a prerequisite to
restaging. No Linux DeviceService route has been enabled.

## 2026-09-29 supervised locked-session gate

The corrected candidate was built natively on `neko-gpu-1` under the same
UID as `neko`; 35 focused daemon tests passed, with the installed locked
retrieval test intentionally ignored until its preconditions could be
observed. A transient same-UID daemon used a private Unix socket. In the
target's graphical Terminal, `neko` entered the OS password into the local
hidden prompt; the CLI returned `uid:1000 pending protected`. Metadata readback
confirmed `PENDING`. Neither the password nor a vault secret was sent over
SSH or printed by the CLI.

The first locking sequence included an accidental logout, so the original
console session 52 ended and a new GNOME Wayland session 302 began. The owner
then locked session 302. A second, clean lock of the same session reproduced
the same read-only result: logind `LockedHint=yes`, while both GNOME Shell's
direct `org.gnome.Shell.ScreenShield.GetActive` and the
`org.gnome.ScreenSaver.GetActive` proxy returned `false`. The session ID,
UID, seat, and creation timestamp remained unchanged during the clean retest.
This difference persisted for over a minute. The installed locked retrieval
test was **not run**, no credential was retrieved while locked, and no unlock
method was called.

GNOME Shell 48.7's [screen shield source](https://github.com/GNOME/gnome-shell/blob/48.7/js/ui/screenShield.js)
sets the logind locked hint from its lock state and tracks its own active state
separately; it allows a visible locked screen while its screensaver interface
reports inactive. The former agreement requirement was invalid for this GNOME
host. The [D-Bus implementation](https://github.com/GNOME/gnome-shell/blob/48.7/js/ui/shellDBus.js)
returns the latter for `GetActive`. The exact reason the target's visible lock
UI left that state false for over a minute is not yet established, but it does
not disprove `LockedHint=true`. `GetActiveTime` increasing does not establish
that the shield is active. `LockedHint` is a desktop-provided hint, not
independent proof of the UI result. The following supervised gate therefore
used the revised logind rule and required the owner to observe the UI result.

After the owner manually unlocked, the target-local CLI removed the
enrollment. An independent Secret Service `SearchItems` for the AUV
application and `uid:1000` returned no item paths. The transient systemd
service was stopped (`ActiveState=inactive`, `MainPID=0`), and its uniquely
named stage, socket, and local transfer archives were removed. No Linux
DeviceService route was enabled during this gate.

## 2026-09-29 revised logind gate

A fresh, uniquely staged candidate used logind `LockedHint` for inventory,
final pre-delivery state, and same-session readback. The exact session ID,
creation timestamp, UID, user, seat, active state, local Wayland type, and
single-session restriction remained in force. Native tests on `neko-gpu-1`
passed: 3 focused Linux driver tests and 35 Device entry tests, with the two
owner-supervised tests ignored in the ordinary run. The target-local CLI again
verified the OS password through PAM and returned only `uid:1000 pending
protected`.

The owner locked session 302. Its ID, UID, seat, and creation timestamp were
unchanged, and `LockedHint=yes`. The ignored
`installed_locked_secret_retrieval_without_unlock` test passed: the same-UID
host retrieved and dropped the protected Secret Service item while the same
session stayed locked, without sending `Unlock`. The separate
`installed_locked_enrollment_unlock_once` test then passed after one logind
`Unlock` request; same-session readback changed `LockedHint` to `no` in 0.38
seconds. The owner independently reported an automatic return to the original
desktop without a password error. This is live evidence for the private host
path on this GNOME configuration, not a paired `DeviceService` or generic
Linux support claim.

After the gate, the target-local CLI removed the enrollment. Independent
Secret Service search returned no AUV item paths, metadata lookup returned
not found, and the transient service was stopped (`ActiveState=inactive`,
`MainPID=0`). The unique stage, socket, and transfer archive were removed.
The owner subsequently chose [PAM revalidation on every Linux remote unlock](https://github.com/moeru-ai/auv/blob/85f638525c669e56aac3ce4792e39292563944c6/docs/ai/references/session-api/2026-09-27-device-entry-credential-decision.md#linux-locked-session-rotation-rule-accepted-2026-09-29).
The candidate now checks the stored password after protected retrieval and
before logind `Unlock`, including for a `READY` enrollment. A confirmed PAM
rejection suspends that generation; service failure does not. The earlier
host gate predates this added check.

## 2026-09-29 locked PAM revalidation gate

A new isolated candidate at `neko-gpu-1` passed 39 Device entry tests; the
two installed tests remained ignored in the ordinary run. The CLI built
natively. A same-UID transient service used a private Unix socket and empty
store. `neko` enrolled through the graphical Terminal's hidden local prompt;
only `uid:1000 pending protected` was returned.

The owner locked the existing physical Wayland session 302. Its UID 1000,
`seat0`, creation time, active state, and `LockedHint=yes` were checked before
delivery. The installed read-only test retrieved the Secret Service item and
validated it through the target's `gdm-password` PAM service while the same
session remained locked (0.12 seconds); it sent no unlock request. A separate
one-shot test repeated the PAM check, requested logind `Unlock` once, and
read back `LockedHint=no` on the same session (0.53 seconds). The owner saw the
original desktop return with no password error. No credential bytes appeared
in the test output. This proves the same-UID host path with a current password
on this installed configuration; it does not yet prove rejection of a rotated
password, the paired API, or a release installation.

## 2026-09-29 paired DeviceService gate

The Linux daemon now routes `ListUserSessions`, `GetUserSession`, and
`EnsureUserSessionUnlocked` through the same target-local policy, account
locks, and audit as `DeviceLocalService`. The native daemon test run passed
65 tests, with two supervised locked-host tests intentionally ignored. Its
paired integration test proved that an authenticated bearer reaches the
disabled policy and writes a paired-device attempt and terminal outcome;
an unauthenticated loopback request cannot reach selection.

An isolated same-UID service on `neko-gpu-1` listened on a private Unix socket
and `127.0.0.1` TCP, with a private pairing store. A temporary client on the
Mac connected through SSH port forwarding with a newly paired bearer. Its
gRPC `ListUserSessions` and `GetUserSession` returned session 302. The owner
then locked that same physical Wayland session. Readback confirmed the same
UID, user, seat, creation time, and `LockedHint=yes`; enrollment was `PENDING`.
The Mac client sent one gRPC `EnsureUserSessionUnlocked` request for the exact
selector. The service returned `UNLOCKED_EXISTING_SESSION`, logind readback
reported `LockedHint=no` on session 302, enrollment became `READY`, and the
durable outcome identified the temporary paired Device. The owner confirmed
that the original desktop returned without a password error. The monitor was
black and failed to wake automatically during this attempt, so this result
does not establish reliable display wake behavior.

The user journal near the request contained no GNOME Shell, Mutter, or display
error. It did include PAM keyring and `pam_unix(gdm-password:account): setuid
failed: Operation not permitted` messages; these did not prevent the observed
PAM check or unlock. The unprivileged SSH account could not read kernel logs,
so the monitor failure has no established root cause from this gate.

After the gate, the target-local CLI removed the enrollment. An independent
Secret Service search returned no AUV item paths, and metadata lookup returned
not found. All three transient units were inactive with `MainPID=0`; the
unique Linux stage, pairing stores, Mac client profile, SSH tunnel, and
transfer archive were removed. This is one configured host and session with a
current password. Rotated-password suspension, display wake reliability, and
release installation remain separate validation gaps.

After the supervised gate, code review found a cancellation race around the
blocking PAM transaction and observed PAM keyring effects before final pairing
authorization. The follow-up keeps PAM synchronous under the existing policy
and account guards, rechecks the same locked session after vault retrieval,
and reauthorizes the exact bearer immediately before PAM. A focused regression
test revokes a bearer during the asynchronous credential read and asserts that
no PAM effect or OS unlock follows. These follow-up changes passed native
Linux tests but were not part of the supervised unlock binary above; a later
installed gate is needed to claim their live behavior.

## 2026-09-29 final review-fix retest

The reviewed source snapshot (SHA-256
`482b2b73d06f3d53c345962b418f96229bb6ec90243aa8393d1dd816dbae09d0`)
was built on `neko-gpu-1`; 66 daemon library tests passed and the two
supervised host tests remained ignored in the ordinary run. Its private
same-UID service listened on a Unix socket and loopback TCP. The owner enrolled
locally through the hidden terminal prompt. A Mac client paired through an
SSH tunnel and listed the unchanged physical Wayland session 302.

The owner locked session 302 twice, without logout. On the first attempt,
`PENDING` enrollment passed protected retrieval and `gdm-password` PAM
revalidation, then the paired gRPC call returned
`UNLOCKED_EXISTING_SESSION`; same-session logind readback changed
`LockedHint=yes` to `no`, enrollment became `READY`, and the owner saw the
original desktop. On the second attempt, `READY` enrollment again produced
`UNLOCKED_EXISTING_SESSION` on the same selector, a new `gkr-pam` journal
record, `LockedHint=no`, and a paired Device audit outcome. The owner reported
that the desktop was unlocked but the monitor stayed black until two physical
mouse clicks. Thus OS session unlock passed twice; automatic display wake did
not pass.

The target-local CLI removed the enrollment. Secret Service `SearchItems`
returned no AUV item paths and metadata lookup returned not found. The
transient service had `ActiveState=inactive` and `MainPID=0`; the unique host
stage, pairing store, Mac client profile, SSH tunnel, and transfer archive were
removed. This retest covers the final cancellation and pre-PAM authorization
fixes with a current password on one configured GNOME host. A rotated-password
rejection, other desktop configurations, and release installation remain open.
