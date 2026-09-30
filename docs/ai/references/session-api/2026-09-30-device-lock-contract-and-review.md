# Device lock contract, validation, and review (2026-09-30)

Status: experimental implementation. This note records the current source and
configuration-specific evidence; it is not a general three-platform support
claim.

## Contract

`DeviceService.EnsureUserSessionLocked` takes exactly one non-empty `user` or
opaque `session_selector`. It selects an existing login instance, shares the
paired-bearer and target-local policy gates with unlock, requires the selected
OS account to have a target-local enrollment, and writes a restricted target
audit attempt and outcome. Lock does not read or submit the credential. The
native host rechecks the exact usable instance, requests OS lock, and the
policy independently reads back that instance as `LOCKED`. A previously
locked selected session returns `ALREADY_LOCKED` without native input.
Successful audit results identify lock effects; denied and canceled records
still use the existing common result names. An explicit operation field needs
a persisted-record and local-read contract change and is deferred at the
`AuditAttempt` call site.

The CLI `devices lock` and MCP `device_ensure_user_session_locked` use the same
typed Rust operation. The protobuf method remains gRPC-only like the initial
unlock method. A validated HTTP request/OpenAPI contract is deferred; see the
inline `TODO(device-entry-http)` at the RPC.

## Evidence levels

| Platform | Native mechanism | Evidence as of 2026-09-30 |
| --- | --- | --- |
| macOS | Signed Aqua helper validates the exact console and posts Control–Command–Q; helper and daemon both read back the selected session. | Swift bridge generation, SwiftPM build, helper and policy tests, installed signed package, one paired `lock → LOCKED → unlock → USABLE` API sequence, and owner-observed return to the original desktop. |
| Linux GNOME | Same-UID logind `Session.Lock` on the exact active session, then same-login `LockedHint` readback. | Native debug build and focused driver/daemon tests on `neko-gpu-1`, reversible daemon update, one paired `lock → LOCKED → unlock → USABLE` API sequence, and owner-observed return to the original desktop. Lock-screen rendering was not observed as a separate stage. |
| Windows | Selected-session LocalSystem worker checks the interactive Default desktop, calls `LockWorkStation`, then host and policy read back WTS lock state for the same login. | Native Windows build and focused driver state test, isolated service/worker deployment, one paired `lock → LOCKED → unlock → USABLE` API sequence for console 5. A separate UI daemon then executed `display.list`, `display.capture` (Run `870850dc-0dd3-8ed0-efdc-a75eab20b660`), and `window.list` (Run `14b456a8-c703-ed84-ab95-d03d64b00ff2`), which returned visible Explorer, Settings, and Windows Terminal windows. The owner could not observe the screen, so direct visual confirmation remains unavailable. A successful `LockWorkStation` call alone is not a verified lock. |

Windows policy tests using the generic user-owned temporary directory do not
run under the production `SYSTEM` ACL contract. The new shared-policy tests
are Unix-gated until a SYSTEM-owned Windows fixture exists; the Windows driver
test and installed paired API gate cover distinct native behavior.

## Pairing and enrollment experience

Pairing is directed: the target's pairing service issues a one-time token; the new
controller consumes it through `devices pair connect` and stores a named local
profile and opaque bearer. Any active bearer paired to that target can request
the target's entry operations. There is no unlock-only or lock-only grant per
controller. The OS user separately enrolls a credential on the target; the
target-local capability switch can disable all entry operations. Revocation,
disable, or unpair removes future bearer authority. The credential never
travels to the controller.

The first token needs a target-local bootstrap, but an existing active paired
bearer can issue a further token remotely. This is the accepted delegation
policy in `2026-09-27-device-unlock-authority-decision.md`; CLI pairing help
currently describes token creation as target-local only and needs correction.

The local controller → `neko-mbp-m1` demonstration from this investigation has
a saved `pr198-macos-demo` profile. It listed the target `neko` session and,
after the new lock deployment, completed a separate paired
`lock → LOCKED → unlock → USABLE` sequence. The
profile's configured name does not currently work as a `--device` selector;
the stable target Device ID does. Treat selection by profile name as a CLI
experience improvement candidate, not a reason to duplicate routing policy.
On 2026-09-30, an existing paired bearer issued a 120-second token for another
isolated local controller identity (`mac-onboarding`). That new identity
completed `lock → LOCKED → unlock → USABLE` for the same target Mac session,
then `window.list` returned 19 windows (Run
`fc7fce15-bc97-db96-0c39-3136af0fb85a`). Its profile file and parent
directory were created with owner-only permissions. The new pairing has the
same broad target authority as the earlier bearer.

For a new controller, the normal flow is:

1. The target owner installs and starts its daemon and, if needed, enrolls the
   target OS account with `auv device-local enroll --user USER --kind
   os-password` in a target-local terminal. The hidden credential prompt stays
   on the target. Windows PIN enrollment uses `--kind windows-pin`.
2. For the first pairing, the target owner runs `auv devices pair create-token
   --ttl 300` against its local daemon. An already paired controller may issue
   a later token remotely under the current delegation policy. The token is
   shown only once.
3. The controller reaches the target API over the chosen private transport and
   runs `auv devices pair --endpoint http://TARGET:PORT connect --token TOKEN
   --label "Controller" --profile target-name`. This saves a bearer in the
   controller's profile store. The current `--token` argument can appear in
   shell history or process listings; a stdin/file input is a candidate next
   slice.
4. A newly enrolled account is `PENDING`. The paired controller may remotely
   `devices lock` its `USABLE` desktop. That operation does not read the stored
   credential or promote enrollment. The first successful `devices unlock`
   probes the locked host credential and promotes the account to `READY`.
   This first-use sequence has a policy regression test; a new enrollment on
   each installed platform has not yet been live-tested with this change.
5. The controller runs `auv devices list`, selects the target by stable Device
   ID, and runs `auv --device-id ID devices sessions`. Subsequent `devices
   lock` and `devices unlock` calls use exactly one `--user USER` or fresh
   `--session SELECTOR`; success requires same-session state readback.

Pairing grants current broad Device authority, not an unlock-specific grant.
The target owner can disable or unpair a controller; removing an OS enrollment
is a separate target-local operation.

## Review findings and next slices

The security and lifecycle decisions in `device_entry/policy.rs` (caller
reauthorization, account lock, generation, audit transaction, exact readback)
and `metadata.rs` (invalidation, vault publication, poison handling) earn their
code. The current 1255-line `policy.rs` has roughly 795 lines in its test
module, covering cancellation, revocation, races, and state transitions; there
is no evidence for deleting them by line count.

1. **Addressed in source, pending installed-host validation:** macOS and
   Windows enrollment now transfer the account guard into the blocking worker
   with the metadata/vault mutation. A cancellation regression test confirms
   a second same-account operation waits for that worker.
2. **Addressed in source, pending installed-host validation:** the Windows
   vault and daemon metadata store share LocalSystem and opened-handle ACL
   checks. Vault read and delete operate on the verified file handle.
3. **Addressed in policy tests, pending live first-use validation:** `PENDING`
   enrollment may remotely lock its usable session; only locked-host unlock
   probes and promotes the enrollment.
4. **Addressed:** `Policy` now holds `Arc<MetadataStore>` directly; the
   single-adapter `EnrollmentStore` trait is gone. The three-platform
   `SessionHost` seam remains.
5. **Addressed:** the macOS and Linux observation adapters live in their host
   modules, Linux and macOS share `device_entry::unix_account`, and Windows
   account-name formatting is `ConsoleSession::account_name`. The Windows
   DPAPI vault and SYSTEM-only storage checks moved from
   `auv-driver-windows` into `auv-daemon` `device_entry`; the driver exposes
   `unlock_with_worker(target, credential)` and no longer reads storage.
6. **P2 Mac helper lifecycle:** its current-thread timeout does not bound a
   synchronous Keychain or native input call. A future fix must retain
   authorization and account locks through input completion.
7. **P3 localized cleanup:** consolidate macOS helper status encode/decode.
   The stale Windows "register only after" comment was removed with the
   driver's vault-reading entry point. The earlier
   one-variant Windows `WorkerMode` concern is resolved by the new lock mode;
   it now selects two real worker operations.

RustDesk provides useful platform confirmation: its
[input path](https://github.com/rustdesk/rustdesk/blob/master/src/server/input_service.rs)
uses Control–Command–Q on macOS and Super+L on Linux; its
[Windows path](https://github.com/rustdesk/rustdesk/blob/master/src/platform/windows.rs)
uses `LockWorkStation`. [Microsoft documents](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-lockworkstation)
that a successful `LockWorkStation` call only starts locking.
Its large input/platform files are not a module-size model for AUV. General
input crates such as Enigo may be evaluated for ordinary input; Device entry
still needs AUV's target-local credential, bearer, session, and audit policy.
RustDesk is [AGPL-3.0](https://github.com/rustdesk/rustdesk/blob/master/LICENCE)
while AUV is Apache-2.0, so copying its source requires a
separate licensing decision.
