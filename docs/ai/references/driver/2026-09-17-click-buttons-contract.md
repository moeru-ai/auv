# Click button integration

Date: 2026-09-17. Classification: approved feature, BG-1 button integration.

## Contract

The owner selected left, right, and middle buttons across the existing macOS,
Windows, and Linux click paths. The existing `MouseButton` type owns the choice.
A click is still a complete down/up operation; hold and drag remain deferred
until their ownership, cancellation, and release contract is approved.

- `ClickOptions.button` selects the button for window clicks and defaults to left.
- Local Rust `input().click_at(point, button, click, modifiers)` and the Rust
  Runner client's `input().click_screen_point(point, button, click, modifiers)`
  take an explicit button argument. Existing callers pass `MouseButton::Left`.
- Protobuf `ClickOptions.button` and `ScreenClickOptions.button` use the new
  `MouseButton` enum. Unspecified means left; unknown numeric values are rejected
  before delivery. RPC names and result schemas are unchanged.
- JS `window.click(point, options)` and `runner.input.clickScreenPoint(point,
  options)` accept the generated button enum through their existing options.
- `input.clickPoint --button left|right|middle` defaults to left. Typed invoke
  arguments retain the choice for Runner dispatch and recorded replay.

Clients and Runners are updated together. The owner explicitly excluded old-peer
compatibility and version negotiation from this experimental API slice.
Omission defaults are ordinary API behavior, not a mixed-version support claim.

Example:

```rust
session.input().click_at(
  point,
  MouseButton::Right,
  Click::Single,
  ClickModifiers::default(),
)?;
```

```sh
auv invoke input.clickPoint 100 80 --button right
```

## Platform boundaries

macOS maps the shared enum to the existing native button codes. Both window
strategies and foreground delivery use that selection. ChromiumCompatible still
uses the existing primer, dual posting, and count cap. BG-2 is unchanged; receipt
of duplicate events is not exactly-once activation evidence.

Windows selects corresponding SendInput down/up flags, including partial-batch
cleanup, and corresponding background down/up/double-click messages and held
button flags. Existing Shift/Control-only background modifier support remains.

Linux Portal and uinput send the selected evdev button code on both press and
release. Existing modifier cleanup, Portal session ownership, and Wayland
background-only refusal remain unchanged.

`InputActionResult` remains delivery evidence with separate semantic verification.
This slice does not add another delivery strategy or change click counts.

## Validation

- macOS independent AppKit receiver: all three buttons with and without modifiers
  passed on PidTargeted, ChromiumCompatible, and foreground delivery. Receiver
  checks button number, down/up type, target window, and modifier state. The
  compatibility route retains the previously observed duplicate events.
- Rust focused suites: 302 passed, 7 ignored across common driver, macOS driver,
  invoke, CLI, and client library unit/binary tests.
- Windows cross compilation: `cargo check -p auv-driver-windows --all-targets
  --target x86_64-pc-windows-msvc` passed. This does not execute native Windows tests.
- `scripts/generate-swift-bridge` and macOS native SwiftPM build passed.
- Buf generation and breaking check against main passed. Buf lint retains the
  existing MoveMouseStreamResponse naming violation at input.proto:35.
- SDK typecheck retains existing `AbortSignal.any` declaration errors in client,
  driver, and daemon sources.
- Workspace all-target checking encounters an existing missing `Config.id` in
  `supported/apps/auv-netease-music/tests/custom_runner_e2e.rs:15`.

The AppKit receiver is opt-in:

```sh
cargo test -p auv-driver-macos --test click_modifiers -- --ignored --nocapture
```

Windows has an opt-in independent Win32 receiver covering all three buttons and
modifier carryover, plus native SendInput partial-delivery cleanup tests.
Linux has an isolated D-Bus receipt fixture covering all three Portal buttons.
Native execution and remote desktop evidence are recorded separately below.

## Remote access evidence and resumption

The existing Cloudflare SOCKS tunnel on the Mac permits kubectl access using
`~/.kube/config.d/ihome-cloudflare.conf`; the direct cluster hostname did not
resolve. Both target nodes were Ready: neko-gpu-1 at 10.0.0.196 and luoling-win11
at 10.0.0.139. The existing SSH aliases pointed at stale addresses. These
addresses are observations, not permanent configuration.

The existing `rc-dev/auv-wayland-validation` Pod on neko-gpu-1 accepts
`kubectl exec`. Its agent-owned Sway session has wayland-1, a session bus, and
PipeWire. It has no Cargo or detected native development pkg-config files.
The Pod can connect to both host SSH ports and receive their OpenSSH banners.
A bidirectional kubectl/Python bridge reached SSH public-key authentication.
No private keys were copied into the Pod.

The first attempt failed at the Mac SSH agent: Linux signing reported
`communication with agent failed`; Windows reported `agent refused operation`.
After the owner requested a fresh authorization, both host logins succeeded.
Linux identified itself as `steam-deck-55d` (user neko); Windows as `LUOLING-PC`
(user luoling8192). The bridge is verified through authenticated remote command
execution, not only an SSH banner.

The initial Windows inspection found no Cargo or logged-in interactive user.
The owner subsequently authorized toolchain installation. Rust/Cargo 1.95.0
(MSVC host), Visual Studio Build Tools 2022 17.14.7, and the Windows SDK were
installed successfully. `vswhere` reported a complete installation with no
reboot required. Interactive desktop receipt remains distinct from hidden
Win32 receiver tests; the successful SSH login alone is not GUI input evidence.

The access route can be reproduced without rcctl or an installed daemon. Save
this local proxy helper, make it executable, and use it as SSH's ProxyCommand:

```sh
#!/bin/sh
export KUBECONFIG="$HOME/.kube/config.d/ihome-cloudflare.conf"
exec kubectl -n rc-dev exec -i auv-wayland-validation -- python3 -u -c '
import os, select, socket, sys
s = socket.create_connection((sys.argv[1], int(sys.argv[2])), 10)
s.settimeout(None)
while True:
    ready, _, _ = select.select([s, 0], [], [])
    if s in ready:
        data = s.recv(65536)
        if not data:
            break
        sys.stdout.buffer.write(data)
        sys.stdout.buffer.flush()
    if 0 in ready:
        data = os.read(0, 65536)
        if not data:
            break
        s.sendall(data)
' "$1" "$2"
```

For example, after refreshing node addresses, set `HostName` to the observed
address and `ProxyCommand='/path/to/proxy %h %p'` on the existing SSH alias.
Keep keys and kubeconfig on the Mac, and retain SSH host-key checking. The
existing tunnel and Pod were reused without modifying their configuration.

Additional local checks passed: `cargo check --workspace`, default `cargo test`
(82 passed, 1 ignored, overlapping the focused CLI tests), `cargo fmt --check`,
`git diff --check`, and the SDK driver request tests (3 passed). The live Mac was
macOS 26.3 (25D2125), arm64.


### Linux host validation after SSH authorization

The test workspace is isolated at `/tmp/auv-buttons-20260917`; the existing
checkout was not modified. It contains the current common/Linux driver sources,
with the workspace members reduced to those crates and the same package metadata.
Rust 1.95.0 was already installed. No packages, services, or daemons were installed.

- `cargo +1.95.0 test -p auv-driver-linux --lib`: **81 passed, 3 ignored**.
- The opt-in `modified_click_reaches_portal_and_cancelled_selection_closes_session`
  test: **passed**. Its independent D-Bus fixture received Linux button codes
  272, 273, and 274, each with press/release, plus modifier release and session
  cancellation. This is Portal protocol evidence, not a live Portal GUI receipt.
- A standalone GTK4 receiver in the host GNOME `wayland-0` session observed the
  current driver's uinput backend. The probe called `click_at` with Left, Right,
  and Middle, a single click, and empty modifiers. The receiver logged exactly:

```text
phase  GTK button
 down  1 (left)
   up  1
 down  3 (right)
   up  3
 down  2 (middle)
   up  2
```

All six events were at approximately `(300, 300)` in the fullscreen test window.
The receiver was terminated after validation. Probe source, receiver source, and
JSON receipts remain in the isolated remote workspace (`live.py`, `receiver.py`,
`crates/auv-driver-linux/examples/buttons_probe.rs`, and `receipts.jsonl`). This
is live evidence for that GNOME/GTK4/uinput environment with empty modifiers;
it does not establish every toolkit, background delivery, or a live Portal path.


### Windows build environment

The owner authorized installing the missing native build environment on
LUOLING-PC. Build Tools was installed through winget's
`Microsoft.VisualStudio.2022.BuildTools` package with the VCTools workload,
recommended components, quiet installation, and `--norestart`. Rustup came from
its official `static.rust-lang.org` MSVC distribution; the default toolchain is
1.95.0 with the minimal profile. The setup follows the
[Rust Windows prerequisites](https://github.com/rust-lang/rustup/blob/main/doc/user-guide/src/installation/windows-msvc.md)
and [Visual Studio installer arguments](https://learn.microsoft.com/en-us/visualstudio/install/use-command-line-parameters-to-install-visual-studio).

Toolchain installation is persistent. Installer logs are under the user's
`%TEMP%\auv-buttons-20260917-setup`. Current test sources were uploaded to
`%TEMP%\auv-buttons-20260917`, with their common and optional path dependencies;
the remote workspace members are restricted to the Windows driver dependency
closure. No existing checkout was modified.

Windows PowerShell 5.1 can treat native stderr informational output as a
terminating `NativeCommandError` with `ErrorActionPreference=Stop`. The initial
Rustup invocation stopped for this shell-level reason. The successful retry
used `Start-Process -Wait -PassThru` with separate stdout/stderr log files and
checked the process exit code. This is a setup detail, not a Rust or AUV failure.


Windows validation after installation:

- MSVC toolset directory: **14.44.35207**; Windows SDK: **10.0.26100.0**.
- `cargo +1.95.0 test -p auv-driver-windows --lib`: **82 passed, 1 ignored**.
  This includes right/middle SendInput event construction and selected-button
  cleanup after partial delivery.
- `cargo +1.95.0 test -p auv-driver-windows
  window_receives_modified_click_messages -- --ignored --nocapture`: **1 passed**.
  The independent hidden Win32 receiver observed corresponding left/right/middle
  messages, Shift/Control state, and subsequent ordinary clicks without carryover.
- Native build and test execution succeeded without a machine reboot. Logs are
  `native-tests.log` and `receiver-test.log` in the isolated Windows workspace.

The Win32 receiver validates posted window messages in its own thread. It does
not establish foreground SendInput receipt in a logged-in desktop, toolkit-wide
consumption, or semantic application success. No interactive login, autologon,
RDP configuration, or extra input daemon was installed by this setup.

### Scoop ownership of Rustup

The owner subsequently selected Scoop as the package manager for Rustup. The
final installation is **Scoop main/rustup 1.29.1**, with Rustup managing the
**1.95.0-x86_64-pc-windows-msvc** toolchain. The MSVC Build Tools and Windows SDK
remain the winget-installed prerequisites above.

Existing Cargo caches and installed toolchains were preserved in Scoop's
persistent directories before running `scoop install rustup`:

- `CARGO_HOME`: `%USERPROFILE%\scoop\persist\rustup\.cargo`.
- `RUSTUP_HOME`: `%USERPROFILE%\scoop\persist\rustup\.rustup`.
- Active `rustup`, `cargo`, and `rustc` commands resolve through
  `%USERPROFILE%\scoop\apps\rustup\current\.cargo\bin` in a fresh SSH session.
- Rustup automatic self-update is disabled; Scoop owns Rustup updates, while
  Rustup continues to manage compiler toolchains and targets.

The obsolete manual `%USERPROFILE%\.cargo\bin` PATH entry and its stale Windows
uninstall registration were removed. The previous manual directories and
registration were retained as a rollback backup under
`%TEMP%\auv-buttons-20260917-setup\manual-rust-backup`; they are not active.

Candidate next slice: repair the pre-existing Scoop Git installation, because
its `current\cmd\git.exe` is missing and Scoop self-update consequently fails.
This did not prevent installing the hash-verified Rustup package from the
existing main bucket. No Git repair or unrelated package upgrade was performed.

After migration, a fresh SSH session resolved all three Rust commands through
Scoop, and the MSVC toolchain remained active. Rebuilding and rerunning the
Windows library tests produced **82 passed, 1 ignored**; the independent Win32
receiver test again passed with Cargo exit code 0. Migration validation logs use
the `scoop-` prefix in the same isolated Windows test workspace.

### Windows button/count receipt follow-up

The independent hidden Win32 receiver also passed all 18 combinations of
left/right/middle, single/double/three repeated clicks, and Shift+Control/plain
input. Assertions cover the exact message sequence and held-button/modifier
flags: double clicks deliver the selected button's `WM_*BUTTONDBLCLK`, repeated
clicks deliver three separate down/up pairs, and the subsequent plain call has
no modifier flags. The focused ignored test passed with Cargo exit code 0;
logs are `button-count-receiver.log` and `button-count-build.log` in the same
isolated Windows workspace. This is posted-message receipt evidence only.

The subsequent desktop check found no logged-in user (`quser`), an unnamed
console session 1, and the SSH process in service session 0. Foreground
`SendInput` receipt was initially blocked pending an owner login and unlocked
interactive desktop. No foreground test was attempted from the service session;
the subsequent RDP receipt evidence is recorded below.


### Windows foreground receipt in an RDP desktop

After the owner logged in as `neko`, the RDP desktop was active in session 50.
The new opt-in integration test
`crates/auv-driver-windows/tests/click_buttons.rs` passed **18 combinations**:
left/right/middle, single/double/three repeated clicks, and Shift+Control/plain
input. The executable completed with **1 passed, exit 0**, in 23.69 seconds.

The test calls the public `auv_driver_windows::input::click_at` API. A separate
Win32 window procedure records actual translated mouse messages and modifier
flags; it does not use CUA to inject or validate clicks. Assertions cover the
selected foreground delivery path, exact receipt sequence, native double-click
messages, and released button/modifier state. Driver `verified` remains false:
receiver assertions are separate verification, not a semantic-success claim by
the driver. Three repeated clicks used 600 ms intervals, beyond the observed
500 ms Windows double-click threshold; double clicks used 50 ms intervals.

The first fixture ran synchronous injection on the receiver thread and observed
only one pair during a three-click case. Keeping the receiver message pump active
while injecting from a worker thread resolved this fixture failure; no production
code was changed. This keeps the receiving application responsive during input.

To reproduce directly in an unlocked Windows desktop:

```sh
cargo test -p auv-driver-windows --test click_buttons -- --ignored --nocapture
```

For the SSH-driven run, the native executable was copied to
`C:\Users\Public\auv-buttons-20260917` and launched using a temporary scheduled
task with the logged-in `neko` interactive token and limited privileges. The
receiver checked foreground ownership before each case, closed after the test,
and restored the pointer on success. Logs remain as `foreground-receipts.log`
and `foreground-exit.txt` in that directory; the temporary scheduled task was
removed after validation.

Evidence level: live foreground input receipt in this Windows RDP session.
Physical-console receipt, other application/toolkit consumption, and semantic
application success remain outside this evidence. Hold/drag and the remaining
BG-2 delivery differences were not changed.

### Linux remote workflow revalidation

After renewed owner authorization of the local SSH agent key, the same
kubectl-exec TCP bridge reached `neko-gpu-1` (`steam-deck-55d`) as unprivileged
user `neko` (UID 1000). Before authorization the server accepted the public key
but the local agent refused signing; this was an authentication boundary, not a
Kubernetes network failure.

The existing seat0 session 126 was active Wayland with `LockedHint=no` and GNOME
ScreenSaver `GetActive=false`. The host was Debian 13.6, GNOME Shell 48.7, GTK
4.18.6, Cargo 1.95.0. Both `wayland-0` (GNOME) and `wayland-1` (Sway) existed;
the probe explicitly selected GNOME with `XDG_RUNTIME_DIR=/run/user/1000`,
`WAYLAND_DISPLAY=wayland-0`, the same user's D-Bus socket, and
`GDK_BACKEND=wayland`, removing inherited `SWAYSOCK`. The user already had write
access to `/dev/uinput`; no account, package, service, or permission change was
needed.

`timeout 90s python3 /tmp/auv-buttons-20260917/live-resume.py` rebuilt the AUV
uinput probe and passed with exit 0. The GTK receiver's readiness marker required
its window to be active. Exact receipt was down/up for GTK buttons **1, 3, 2**
(left, right, middle), at approximately `(300, 300)`. The wrapper terminated the
receiver in `finally`; no receiver process remained after the run. Source and
logs remain in the isolated remote directory as `live-resume.py`,
`live-resume.log`, `receiver.py`, `receipts.jsonl`, and
`crates/auv-driver-linux/examples/buttons_probe.rs`.

Local and remote SHA-256 matched for the relevant implementation files:

- Linux `src/input.rs`: `3a9f3265a5dd80857ee4833081f5c3da11e7589a79f588e6a06a4cbe741cfcf9`.
- Linux `src/native/uinput.rs`: `ab6b43b4faa6a1f8cdc68d0f6a7a0a67c6a19f13528b197a9ce19ad5e1bb2340`.
- Common `src/input.rs`: `55a4c85c0a87f20786b0d66922bb5a68c61a7cd3ec3fc6856f4563be862c671d`.

This revalidation establishes live AUV uinput receipt for three unmodified single
clicks in the named GNOME/GTK environment. It does not extend the earlier evidence
to live Portal input, modified or repeated clicks, capture, GPU rendering, or
semantic application success. RDP, VNC, and CUA were not needed: SSH-launched
Wayland clients connected to the existing user's compositor directly.
