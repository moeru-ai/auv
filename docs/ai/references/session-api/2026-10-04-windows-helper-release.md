# Windows helper release packaging

Status: implemented release packaging contract as of 2026-10-04; privileged
installation remains a separate gate.

The Windows x86-64 release archive contains two version-matched Rust binaries:

```text
auv.exe
auv-helper.exe
```

`auv.exe` remains the public CLI and the executable registered for the
`AuvDevice` LocalSystem service. `auv-helper.exe` is an internal one-shot
helper. The service resolves it beside its own executable and launches it in
the selected console session for the existing-session lock and unlock paths.
It is not a second daemon or a user-facing command.

The helper is an explicit `auv-driver-windows` Cargo binary target. Windows CI
builds that target, and release CI builds it with the same Cargo lockfile,
target, source revision, and release profile as `auv.exe`. Release CI also
checks that direct invocation is rejected before archiving both executables.

The public Rust module remains `device_unlock_host` for source compatibility.
Its name predates the lock route; changing that exported module requires an
explicit Rust API migration and is not part of release packaging.

This change does not claim a clean-host installation path. Registering the
SCM service, protecting the installation and ProgramData roots, issuing the
offline pairing bootstrap token, enrolling a target-local credential, and
performing an installed lock/unlock gate remain the installation slice. A ZIP
containing the helper is necessary for that slice but does not itself satisfy
those security and behavior gates.
