# Windows locked-session host handoff

Status: one installed-host, owner-observed locked-console gate passed on
2026-09-29. Isolated Windows native builds and focused unit tests passed for
the driver, service mode, DeviceLocalService transport, and restricted
persistence. A temporary LocalSystem service accepted target-local PIN
enrollment, listed the existing console session through the paired Device API,
and returned `UNLOCKED_EXISTING_SESSION` for that same locked login. WTS
readback changed the same selector to `USABLE`; the owner confirmed the
original desktop with no PIN error. The PIN enrollment and temporary service
were subsequently removed. This is configuration-specific live evidence, not
a general Windows release or installer claim.

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

## Owned code and boundary

- `auv-driver-windows::device_session` observes one physical-console login and
  performs one locked-session credential submission inside a LocalSystem worker
  already placed in that session.
- `auv-daemon` `device_entry::vault_windows` stores a credential by stable
  account SID in a machine-scope DPAPI blob under the OS ProgramData folder's
  `AUVDeviceEnrollments` leaf. It requires a LocalSystem process in Session 0,
  a SYSTEM-owned, SYSTEM-only protected directory and file DACL, and rejects
  reparse points. Machine-scope DPAPI does not itself restrict which local
  principal can decrypt; the file ACL and process identity are required. Its
  SYSTEM-only object checks are shared with policy, audit, and pairing
  storage in `device_entry::storage_windows`.
- `auv-driver-windows::device_unlock_host::unlock_with_worker` accepts a
  validated `ConsoleSession` and a credential that the daemon host has just
  retrieved from its vault, and resolves the worker executable beside the
  installed service process. It duplicates the SYSTEM token
  into the selected console session, creates a random one-shot local named
  pipe with a SYSTEM-only DACL, verifies that the connected client's PID is the
  newly started worker, and sends a fixed-size in-memory credential frame.
  The host checks the exact locked login before vault retrieval and again
  before transfer; the worker revalidates the login before delivery. Both check
  WTS state afterward. No credential is placed in arguments, environment,
  files outside the protected vault, logs, or typed Device results.

The worker executable was originally named `auv-device-unlock-worker.exe`; the
release artifact now ships it as `auv-helper.exe`. It is a separate binary
because the current `auv-daemon` crate is a library. The CLI now has
an opt-in Windows SCM service mode, installed temporarily for this gate. Ordinary
foreground `auv serve` keeps Device entry unsupported. The SCM candidate
binds the Windows policy only under its explicit `enable_device_entry` flag.

`auv-api-server` now has a **candidate** dedicated DeviceLocalService named-pipe
listener. It verifies the connected client SID after each successful pipe read
and carries that verified SID to the management RPCs. The pipe grants
Authenticated Users only `READ_CONTROL`, `SYNCHRONIZE`, `FILE_READ_ATTRIBUTES`,
`FILE_READ_DATA`, and `FILE_WRITE_DATA`; LocalSystem retains full control. A
Windows-native `CreateFileW` gate denied `0x00120003` but admitted `0x00120083`.
The latter still excludes
`FILE_CREATE_PIPE_INSTANCE`: Microsoft's [named-pipe security guidance](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)
states that generic write includes that right. The SCM candidate binds it
separately from paired routes. The Windows client requests only the data,
read-control, and synchronization rights, then verifies the connected server's
LocalSystem token and Session 0 before it exposes a gRPC channel. The CLI reads
a PIN from an attached console with echo disabled. The installed pipe has
answered read-only and PIN enrollment requests; no PIN was sent through the
paired API or included in a typed result.

Non-secret policy, enrollment, audit, and pairing data for SCM mode use the
fixed `ProgramData\AUVDeviceEntry` root. The LocalSystem-only store checks
owner, protected DACL, file type, and reparse state on opened handles, and
holds a checked root handle while operating. The ordinary Windows daemon uses
its preexisting pairing path and does not construct Device entry state.
SCM mode restricts its paired listener to loopback and disables first-party
and custom Runners. A dedicated Device-only paired router is still deferred.
The local transport verifies an enabled administrator group in the caller
token. Management permits self-service by the verified console account SID;
administrator authority is restricted to the selected live console account.

## Installation and supervised gate

The host installer needs to place both the serving executable and worker in an
administrator-owned installation directory that an ordinary user cannot
replace, register the serving process as a LocalSystem Windows service, and
permit it to create a console-session worker. The local management service must authenticate
the real pipe peer SID before allowing `enroll` or `remove`; an ordinary user
may manage only their own SID, while an administrator may manage another SID.

Run this gate on an owner-observed Windows console after code review:

1. Record exact build revision, signed or hashed installer and worker binaries,
   current WTS session ID, logon time, account SID, lock state, service token
   SID, and service session ID. Reject an RDP or disconnected session.
2. Install the service and worker without placing a credential in command line,
   environment, shell history, task XML, event log, or trace. Verify service is
   LocalSystem in Session 0 and the worker path is protected from replacement.
3. Enroll through the authenticated target-local management path. Treat a
   successful write and immediate readback as `PENDING`. Inspect the vault
   directory and item owner/DACL; both must be SYSTEM-only. Do not print blob
   or credential content.
4. Lock the *existing* console login while the owner watches. Independently
   confirm WTS reports that same session, logon time, and SID as locked.
   `verify_while_locked` must read the item under the installed service
   identity before enrollment can become `READY`.
5. Request one unlock through the actual Device API. Confirm one worker starts
   in the selected console session as LocalSystem, the pipe client PID matches,
   and WTS reports the same account and login as usable. Require owner-visible
   desktop confirmation. An accepted `SendInput` count alone is insufficient.
6. Check audit and trace allowlists for credential bytes, key values,
   password-field images, and secret-derived error text. Re-test disabled,
   removed, stale-selector, unknown-state, and already-usable outcomes without
   sending any credential.

Rollback must stop the unique service, terminate its one-shot worker if still
running, remove the service registration and installed files, remove the
protected vault entry through the authenticated local path, and verify the
service, worker, pipe, and item are absent. Preserve only redacted gate status.

## Current validation limit

`cargo check -p auv-driver-windows --target x86_64-pc-windows-msvc` passes.
The Windows node is reachable at `10.0.0.139:22` through the existing
`rc-dev/luoling` Kubernetes pod and `~/.kube/config.d/ihome.conf`. The SSH
alias's old address `10.0.0.132` timed out. The owner has since logged
`luoling-pc\\luoling8192` into the physical console; read-only `query session`
showed session 5 `Active`. The owner confirmed this account can use a PIN at
the local lock screen and entered it only in the target-local hidden prompt.

Isolated Windows native snapshots passed `auv-api-client` verified-pipe tests,
`auv-api-server` named-pipe tests, `auv-daemon` Windows host/enrollment tests,
restricted storage tests (4/4, including reparse rejection), SID-filtered
audit tests (2/2), and protected pairing tests (2/2). The ordinary Windows
foreground server now skips LocalSystem-only Device entry state; SCM mode
opts in explicitly. The combined binary build, LocalSystem service and paired
session-list roundtrip, installed named-pipe policy read, and target-local PIN
enrollment have passed. Native unit results alone do not prove locked-session
behavior.

The first installed PIN enrollment stopped at `suspended`: the vault directory
had owner `BA` and no child. Its creation descriptor specified only a SYSTEM
DACL, while the verifier required owner `SY`. The temporary service was stopped,
that exact empty directory was removed by a one-shot SYSTEM task, and the
descriptor was changed to specify `O:SY`. The second enrollment reached
`pending protected`; read-only inspection found one 230-byte DPAPI item with
SYSTEM ownership and a SYSTEM-only DACL. No blob bytes or PIN were read by the
diagnostic. A Windows-native descriptor-owner regression test passed.

On 2026-09-29, the owner locked the existing physical console session 5. The
paired session list reported the same login selector
`windows-console:5:134351421011070711` as `LOCKED`. One paired
`EnsureUserSessionUnlocked` request returned an unverified outcome in about
233 ms. The same selector remained `LOCKED`, and the owner still saw the
`luoling8192` lock screen. The enrollment became `ready protected`, which
shows that the installed LocalSystem service completed locked-time vault
retrieval. The audit recorded one `attempt` and one `OUTCOME_UNVERIFIED`
outcome. No second credential attempt was made. The current worker collapses
early failures to one exit status, so this evidence does not identify the
failed stage or show that the PIN was rejected. A no-secret worker preflight
must distinguish session/token validation and input-desktop access before
another credential attempt is considered.

The read-only preflight was compiled on the Windows target and reviewed. Its
diagnostic host mode requires LocalSystem in Session 0, launches the same
protected worker executable in the selected console session, and checks token,
unchanged locked login, input-desktop name, and fresh-thread desktop binding.
It does not access the vault or post input. After the owner locked the same
console login, the one-shot SYSTEM task returned `preflight=ready_default
code=0`. Thus worker placement, SYSTEM/session identity, locked-login check,
and binding the `Default` input desktop passed. The task, script, and result
were removed, and the original worker hash was restored and independently
checked. This preflight does not test the one-shot secret pipe, the transition
to `Winlogon`, or PIN submission. The earlier unverified result remains open.

A second reviewed, no-secret diagnostic repeated the restricted one-shot pipe
path, PID match, and fixed 258-byte read/write with a public marker. On the
same locked console login, its SYSTEM task returned `preflight=ready_default
code=0`; the worker read the full marker and passed the identity and desktop
checks. No vault retrieval, PIN decoding, or input occurred. The task and
result were removed, and the original worker binary hash was independently
restored. These two diagnostics narrow the first failed attempt to the
post-transfer worker path, principally the `Default` to `Winlogon` transition,
input delivery, or subsequent outcome readback. A separate harmless Return
transition gate is needed before another enrolled-PIN request.

The reviewed one-Return transition gate ran once on the same locked console
login. It stopped at `transition_return_rejected code=41` before the
`Winlogon` polling and binding phases. The diagnostic sends a Return down/up
pair; if only the down event is accepted, it makes one best-effort release and
still reports failure. The code does not currently distinguish a zero-event
from a one-event insertion in its safe result, so the precise `SendInput`
outcome is unknown. It did not access the vault or send PIN input. The owner
confirmed that the lock background remained unchanged. The one-shot task was removed,
the original worker hash was restored, and the same console login remained
`LOCKED`. At that point, another enrolled-PIN request was deferred until a
separately reviewed input path passed.

The successful throwaway SYSTEM rebind probe had requested desktop access
`0x000F01FF`, while the first Rust transition gate requested only
`DESKTOP_READOBJECTS | DESKTOP_WRITEOBJECTS` (`0x81`). A reviewed diagnostic-only
A/B worker repeated the gate with the prototype's full access on the same
locked console login. It returned `transition_ready code=3`: Return switched
the input desktop to `Winlogon`, and a fresh thread could bind it read-only.
The same login remained `LOCKED`; no PIN was read or entered. The task was
removed and the original worker hash restored. The owner subsequently
confirmed that this harmless Return displayed the PIN input UI. This
comparison implicates the requested desktop access in the Return failure on
this host, but Microsoft's desktop-rights and `SendInput` contracts do not
state that full access is universally required.

The production worker's input-delivery desktop binding was changed to
`0x000F01FF`; read-only observation and worker preflight retain `0x81`.
The new serving and worker binaries were built on the Windows target and
checked by SHA-256 before replacing the temporary installation, with the old
binaries retained for rollback. The service restarted as LocalSystem. Before
the final request, the paired API observed the original
`windows-console:5:134351421011070711` selector as `LOCKED`, and target-local
metadata showed `ready protected`. One Mac-originated paired `devices unlock`
request then returned `UNLOCKED_EXISTING_SESSION`. A subsequent paired read
showed the same selector as `USABLE`; audit recorded one successful outcome
for that attempt; the owner saw the original `luoling8192` desktop with no PIN
error. This validates the installed candidate on this host and login only.

The target-local `remove` command deleted the enrollment. A subsequent `get`
returned not found, and the protected vault directory was empty. The original
binaries were restored, then the task-owned service, installation, store,
vault, bootstrap directory, and scheduled tasks were removed. An independent
read-only check found all those objects absent. The candidate still needs a
release installation path and broader Windows configuration testing before a
general support claim.
