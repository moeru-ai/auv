# Windows Helper and daemon split

Status: implemented on 2026-10-06. Native Windows unit tests and one installed
gate passed on a single Windows 11 (build 26100) host that had the 0.0.28
`AuvDevice` installation. This is configuration-specific evidence, not a
release support claim.

This note supersedes the service shape in
[Windows SCM daemon host](2026-09-29-windows-scm-host.md) and
[Windows Helper installation lifecycle](2026-10-04-windows-helper-install.md).

## Problem

In 0.0.28, `auv setup windows-helper install` registered `AuvDevice`, which ran
`auv.exe serve --windows-service` as LocalSystem. Installing the Helper also
installed a complete daemon: a fixed loopback HTTP listener, a SYSTEM-only
pairing store, Device policy, audit, and a first-pairing bootstrap token. An
ordinary `auv serve` had Device entry disabled, so it could not use an
installed Helper. A request for a LAN listener therefore became a request to
expose the privileged Helper. PR #235 tried that and was closed on 2026-10-05.

## Boundary

```text
remote or local AUV client
        |  Device API + paired bearer
        v
ordinary `auv serve` (runs as the logged-in user)
  owns listeners, pairing, Device policy, enrollment metadata, audit
        |  \\.\pipe\auv-helper, versioned private protocol
        v
AUV Helper Host: `auv-helper.exe --service` (LocalSystem, Session 0)
  owns console observation, the PIN vault, and worker placement
        |  one-shot protected pipe, selected console session
        v
`auv-helper.exe` worker: lock or unlock and verify
```

- **Daemon.** `auv serve` is unchanged on Linux and macOS. On Windows it now
  always opens Device entry state in its own store,
  `<store-root>/control/device-entry`, and serves `DeviceLocalService` on a pipe
  that only its own account can open. Pairing uses the ordinary store; the
  SYSTEM-only pairing mode is removed. The daemon also binds its owner pipe
  beside `--listen http://...` listeners, so a LAN daemon can issue its first
  pairing token without the 0.0.28 bootstrap token file.
- **Helper Host.** The crate `auv-device-helper-windows` builds
  `auv-helper.exe`. With `--service` it runs the SCM `AuvHelper` service. The
  same executable is the one-shot worker that the host starts in the selected
  console session. The host has no network listener, pairing store, policy, or
  audit.
- **Lifecycle.** `auv setup windows-helper install|status|uninstall` manages only
  the Helper. A separate daemon lifecycle (for example a provisional
  `auv daemon install`) is not part of this slice.

## Scope decision

The owner decided that v0 covers an existing physical-console login that is
logged in but locked. The daemon runs as that user, in the foreground or through
a user-owned launcher. A daemon that is reachable after a reboot with no user
logged in is out of scope: it needs a login capability, not
`UnlockExistingSession`. RDP and multi-session selection remain deferred, as
before.

## Helper protocol

Pipe: `\\.\pipe\auv-helper`. One request per connection. Each frame is
`"AUVH" | protocol u16 | operation-or-status u8 | length u16 | payload`, with
little-endian integers and a payload of at most 1024 bytes. The protocol
version is 1. A different version returns `ProtocolUnsupported`, and the daemon
reports `HOST_INCOMPATIBLE`. Accepting a version range is deferred until a
second version ships (`TODO(windows-helper-protocol-range)`).

| Operation | Payload | Host action |
|---|---|---|
| `Observe` | none | Read the physical-console login. Only LocalSystem can read a session token SID, so the daemon cannot observe the console alone. No secret is returned. |
| `Enroll` | account SID, PIN | Write the PIN to the SYSTEM-only DPAPI vault. |
| `Probe` | session ID, logon time, account SID | Prove vault retrieval while that exact login is locked. Success lets the daemon promote `PENDING` to `READY`. |
| `Remove` | account SID | Delete the vault item. |
| `Unlock` | session ID, logon time, account SID | Re-observe the login, retrieve the PIN, run the worker, and read back `usable`. |
| `Lock` | session ID, logon time, account SID | Re-observe the login, run the worker, and read back `locked`. |

## Authorization

- The client opens the pipe with `SECURITY_IDENTIFICATION`. Before it sends any
  byte, it verifies that the kernel-reported server process is LocalSystem in
  Session 0. A process that squats the pipe name cannot receive a PIN.
- The host creates the first instance with `FILE_FLAG_FIRST_PIPE_INSTANCE` and
  `PIPE_REJECT_REMOTE_CLIENTS`. Authenticated users receive only data,
  attribute, and synchronize rights (`0x00120083`), and never
  `FILE_CREATE_PIPE_INSTANCE`. The integrity label is Medium.
- After the host reads a request, it impersonates the client to read the caller
  SID. It rejects anonymous and LocalSystem callers. Every account-scoped
  request is served only when its target SID equals the caller SID. A daemon
  can therefore enroll, probe, lock, or unlock only its own user's console
  login. Cross-account administrator actions are deferred
  (`TODO(windows-helper-admin)`).
- Requests run one at a time. A worker launch or vault write cannot interleave
  with another one.

### Threat model

The Helper Host cannot tell the AUV daemon apart from any other process that
runs as the same Windows user. Such a process can read the daemon's memory and
files, so no pipe-level capability would separate them. Any process running as
the enrolled user can therefore call `Observe` and then `Unlock` that user's
own login directly, bypassing daemon pairing, the remote-entry policy switch,
and daemon audit. The Helper guarantees only this: no caller can act on another
account's login or PIN, and no caller receives a PIN. Separating the daemon
from same-user processes would need a daemon running under its own service
identity, which is the deferred daemon-lifecycle design.

## Storage and 0.0.28 installations

| State | Owner | Location |
|---|---|---|
| Pairing, policy, enrollment metadata, audit | daemon | `<store-root>`; the default store is under the user profile and inherits its private ACL |
| Enrolled PIN | Helper Host | `%ProgramData%\AUVDeviceEnrollments` |

The 0.0.28 Helper had no released users, so the owner decided on 2026-10-06 not
to migrate it. `install` refuses while the 0.0.28 `AuvDevice` service exists.
`uninstall` removes it, together with the old `auv.exe` copy and bootstrap token
directory, but only when its exact recorded command, account, and startup type
prove that AUV registered it. The 0.0.28 SYSTEM-only `%ProgramData%\AUVDeviceEntry`
pairing and policy store is left untouched and is no longer read. Clients pair
again with `auv serve`, and the user runs `auv devices credentials enroll`
again (named `auv device-local enroll` before 2026-10-10).
Replacing an installed `AuvHelper` in place is deferred
(`TODO(windows-helper-upgrade)`): uninstall it, then install again.

The Helper Host reports SCM `Running` only after its LocalSystem identity check
passes and its first pipe instance exists. `install` treats `Running` as
success and fails at once if the service stops during startup.

`stop` waits for the service process to exit after SCM reports `Stopped`.
Without this wait, an immediate removal of `auv-helper.exe` failed with
`ERROR_ACCESS_DENIED` during the installed gate.

## Known limits

- Device entry through the Windows owner pipe still requires per-request SID
  admission (`TODO(device-entry-windows-local)` in `auv-api-server`). Device
  entry uses a paired bearer, also from the same host.
- The daemon store has no Windows owner or mode check like the Unix 0700
  check. It relies on the user-profile ACL. A process that runs as the same
  user can edit its own policy and metadata files. The Helper still limits any
  effect to that user's own login and PIN.
- Root `cargo test` runs only the default member (`auv-cli`). The daemon,
  driver, and Helper unit tests on Windows run only when you name those
  packages.

## Evidence

All results are from one Windows 11 host. The host ran a native debug build
with `AUV_WINDOWS_HELPER_EXE_PATH` set to the built helper:

- `cargo test -p auv-device-helper-windows -p auv-daemon -p auv-api-client
  -p auv-api-server -p auv-driver-windows`: all passed (13, 44, 8, 15, 97 + 1
  ignored).
- (In-place migration, later removed.) `setup windows-helper status` on the
  0.0.28 install reported `degraded` with `legacy_service: installed`.
  `install` returned `ready`. After install,
  `AuvHelper` ran as LocalSystem in Session 0 with
  `"…\auv-helper.exe" --service`, `AuvDevice` was gone, `%ProgramFiles%\AUV`
  contained only `auv-helper.exe`, and nothing listened on 9847.
- An ordinary `auv serve --no-register --listen http://127.0.0.1:19847` bound
  both the HTTP listener and an owner pipe. A token from the owner pipe paired
  a profile over HTTP. Through that bearer, `devices sessions` listed the
  physical console login through `Observe`.
- With a `PENDING` metadata fixture for the account that already had a vault
  item, `devices lock` returned `LOCKED_EXISTING_SESSION`. A following
  `devices unlock` probed, promoted the enrollment to `READY`, and returned
  `UNLOCKED_EXISTING_SESSION` with a `usable` read-back. An unlock issued
  about 2 seconds after the lock returned `OUTCOME_UNVERIFIED`. A retry about
  10 seconds later succeeded, and a separate lock, 10-second wait, and unlock
  cycle passed. The worker path is unchanged by this split.
- With `AuvHelper` stopped, the daemon kept serving and `devices sessions`
  returned `SERVICE_UNAVAILABLE`. The request succeeded again after a restart.
- `install` while the Helper was installed was refused. `uninstall` followed by
  `install` passed, and the vault directory was kept.

Not exercised: a second local account (the cross-SID rejection has unit-test
coverage only), a clean host without 0.0.28, ARM64, and release-signed
binaries.
