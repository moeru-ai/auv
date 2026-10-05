# Windows SCM daemon host

> [!IMPORTANT]
> Superseded on 2026-10-06 by
> [Windows Helper and daemon split](2026-10-06-windows-helper-daemon-split.md).
> `serve --windows-service`, the `AuvDevice` service, the SYSTEM-only daemon
> store, and the bootstrap pairing token no longer exist. This note records the
> 0.0.28 design and its evidence.

Status: Windows service entrypoint implemented; native Windows compile, focused
unit tests, and combined binary build passed. A uniquely named temporary
installation started as LocalSystem in Session 0 and answered paired session
inventory and target-local policy calls. PIN enrollment reached
`pending protected` under SYSTEM-only vault storage. At this recorded stage, a
locked-console unlock remained pending; the later successful gate is recorded
in the [Windows locked-session handoff](2026-09-28-windows-locked-session-host-handoff.md).

`auv serve --windows-service` dispatches the existing daemon through the Windows
Service Control Manager under service name `AuvDevice`. The process checks that
its token is LocalSystem in Session 0. It reports `StartPending`, binds the
daemon, then reports `Running`; SCM `Stop` cancels the daemon and reports
`StopPending` then `Stopped`. Startup or serving failure reports a nonzero
`Stopped` status. Ordinary `auv serve` retains foreground Ctrl-C shutdown.
The SCM path does not depend on a console stdout stream to report readiness.

The service mode requires one explicit loopback HTTP listener and absolute `--store-root`
and `--pairing-store` paths. The store root is fixed to the OS ProgramData
folder's `AUVDeviceEntry` leaf, shared with the Windows Device entry storage;
the pairing path is its `pairings.json` file. The Windows PairingStore creates
or opens that fixed directory under LocalSystem and verifies its SYSTEM-only
security descriptor before reading pairing state.
Service discovery publication, idle shutdown,
and all Runner runtimes, including the first-party local Driver Runner, are
disabled for this host. The service
validates its configured path and LocalSystem identity; the daemon storage
layer creates or validates the directory DACL.
Windows Device entry state is enabled only for this SCM mode; ordinary
foreground `auv serve` continues without the privileged Device entry store.
The Windows `PairingStore` now uses the same protected directory and file
operations as Device entry storage. The native file tests and installed
service identity/listener checks passed; at this stage the locked-console gate
remained open.

The paired loopback listener cannot issue its own first token: that RPC
requires a paired bearer. Before starting `AuvDevice`, an installer must run
the hidden `auv windows-bootstrap-pairing-token` command as LocalSystem in
Session 0. It opens the fixed protected PairingStore while the service is
stopped, issues one twenty-minute token, and writes the plaintext once to
stdout. The reviewed installer redirects it only into a target-local file with
an Administrators-and-SYSTEM-only ACL, then unregisters its temporary SYSTEM
task and starts the service. The token file is removed immediately after
pairing consumes it. No token belongs in a command line, environment variable,
task definition, log, or remote transcript. The PairingStore lifetime lock
rejects an attempted concurrent bootstrap while the daemon holds the store.

## Service registration shape used by the temporary gate

The following is the command shape for an administrator PowerShell session
after the Windows `PairingStore` gate is closed, offline bootstrap has completed,
and `auv.exe` and `auv-helper.exe` have been placed in a
protected `C:\Program Files\AUV` directory. The service creates
`C:\ProgramData\AUVDeviceEntry` with SYSTEM ownership and the protected
`O:SYD:P(A;;GA;;;SY)` security descriptor required by `storage_windows`.
Windows reports the filesystem ACE as `FA` when it is read back; the verifier
accepts only that corresponding protected SYSTEM-only form.
Verify the effective ACLs after starting. The service accepts only loopback
binding for the supervised gate. A dedicated Device-only router and owner
approval are required before broader network exposure.

```powershell
$image = '"C:\Program Files\AUV\auv.exe" serve --windows-service --listen http://127.0.0.1:9847 --store-root "C:\ProgramData\AUVDeviceEntry" --pairing-store "C:\ProgramData\AUVDeviceEntry\pairings.json"'
New-Service -Name AuvDevice -BinaryPathName $image -StartupType Manual
sc.exe qc AuvDevice
sc.exe start AuvDevice
sc.exe queryex AuvDevice
# After the supervised gate:
sc.exe stop AuvDevice
sc.exe delete AuvDevice
```

Manual startup keeps this gate out of boot startup. A controlled installer
prepares a restricted token file before the `New-Service` invocation above.
The installed binary and
worker directory must also be protected from ordinary-user replacement. Keep
the pairing store private; it contains durable authentication state. At this
stage, the API Device unlock route, Windows target-local enrollment, worker
placement, and locked-console result still required their own installed
behavior gate. SCM `Running` proves listener startup, not a successful unlock.
The temporary installer must
refuse a preexisting fixed store, enrollment vault, service, or installation
directory. Its cleanup must verify a SYSTEM-written ownership marker and the
known store contents before deleting either protected root.
