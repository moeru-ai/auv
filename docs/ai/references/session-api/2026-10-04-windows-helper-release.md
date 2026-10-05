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
Since the [Windows Helper and daemon split](2026-10-06-windows-helper-daemon-split.md),
`auv-helper.exe --service` is itself the LocalSystem Helper Host. The host
launches the same executable as the one-shot worker in the selected console
session. It is not a daemon or a user-facing command.

The helper is the `auv-helper` binary target of `auv-device-helper-windows`,
with the AUV Helper icon embedded as a Windows resource. Windows CI builds that target
first, then builds `auv.exe` with `AUV_WINDOWS_HELPER_EXE_PATH` pointing at the
version-matched payload. Release CI uses the same Cargo lockfile, target,
source revision, and release profile for both and checks that direct helper
invocation is rejected before archiving only `auv.exe`.

The checked-in `assets/auv-helper.png` and `assets/auv-helper.ico` are generated
from the same `AUV Helper.icon` source used by the macOS app. Xcode 26 `actool`
renders the Icon Composer document, and `sips` converts its ICNS result to the
256-pixel PNG and Windows ICO. The PNG is the reviewable source rendering; the
ICO is the PE resource consumed by `auv-device-helper-windows/build.rs`.

The public Rust module remains `device_unlock_host` for source compatibility.
Its name predates the lock route; changing that exported module requires an
explicit Rust API migration and is not part of release packaging.

Embedding does not itself satisfy the clean-host behavior gate. Registering
the SCM service, protecting the installation and vault roots, enrolling a
target-local credential, and performing an installed lock/unlock gate remain
separate evidence.
