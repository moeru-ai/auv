# Windows helper release packaging

Status: implemented single-binary release packaging contract as of 2026-10-05;
privileged installation remains a separate gate. The installed lifecycle is
tracked in [Windows Helper installation lifecycle](2026-10-04-windows-helper-install.md).

The Windows x86-64 and ARM64 release archives each contain one public binary:

```text
auv.exe
```

`auv.exe` embeds the architecture-matched `auv-helper.exe` payload. The setup
command extracts that payload into the protected `%ProgramFiles%\AUV`
installation; users and package managers do not install a companion file.
`auv-helper.exe` remains an internal one-shot helper. The service resolves it
beside its installed executable and launches it in the selected console
session for the existing-session lock and unlock paths. It is not a second
daemon or a user-facing command.

The helper is an explicit `auv-driver-windows` Cargo binary target with the AUV
Helper icon embedded as a Windows resource. Windows CI builds that target
first, then builds `auv.exe` with `AUV_WINDOWS_HELPER_EXE_PATH` pointing at the
version-matched payload. Release CI uses the same Cargo lockfile, target,
source revision, and release profile for both and checks that direct helper
invocation is rejected before archiving only `auv.exe`.

The checked-in `assets/auv-helper.png` and `assets/auv-helper.ico` are generated
from the same `AUV Helper.icon` source used by the macOS app. Xcode 26 `actool`
renders the Icon Composer document, and `sips` converts its ICNS result to the
256-pixel PNG and Windows ICO. The PNG is the reviewable source rendering; the
ICO is the PE resource consumed by `auv-driver-windows/build.rs`.

The public Rust module remains `device_unlock_host` for source compatibility.
Its name predates the lock route; changing that exported module requires an
explicit Rust API migration and is not part of release packaging.

Embedding does not itself satisfy the clean-host behavior gate. Registering
the SCM service, protecting the installation and ProgramData roots, issuing
the offline pairing bootstrap token, enrolling a target-local credential, and
performing an installed lock/unlock gate remain separate evidence.
