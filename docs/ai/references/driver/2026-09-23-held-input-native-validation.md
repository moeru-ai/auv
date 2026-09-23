# Held input native validation

Date: 2026-09-23 (Asia/Tokyo). Classification: test-only, with evidence documentation.

The owner requested Linux and Windows validation through the existing Kubernetes
and SSH route. Production input code was unchanged. Source base:
`d2e4d8da3c2c6f462027ff4a77177d0b1459f9e9`, plus the receiver tests in this change.
The isolated remote workspaces retain the original Cargo.lock and a reduced root
workspace containing the two drivers and their path dependency closure.

## Results and limits

| Route | Evidence | Result |
| --- | --- | --- |
| Windows posted messages | Independent hidden Win32 receiver, SSH service session | 15 cases passed |
| Windows SendInput | Independent Win32 receiver in unlocked RDP session 2, user `neko` | 15 cases passed |
| Linux uinput | Separate GTK4 receiver process in GNOME Wayland session 126, user `neko` | 15 cases passed |
| Linux Portal | Independent service on a private D-Bus, public driver API in a child process | 12 held cases passed; one retained session and final close |
| Linux live Portal | Session service introspection | Blocked: no `org.freedesktop.portal.RemoteDesktop` interface |

Each live/message route covers left/right/middle × cross-call down/move/up,
complete timed hold, watchdog release without a subsequent request, cancellation
of a cross-call hold, and complete sampled drag. Assertions require exactly one
down/up pair, the selected button, release coordinates, and button state on
held motion. Repeated up after known cleanup must not post another release.
The Portal fixture covers cross-call down/move/up, timed hold, timeout and
cancellation for all three buttons. Its exact sequence checks one selection/start,
motion on stream 7, matching button transitions, and one session close.

The [receipt artifact](evidence/2026-09-23-held-input/receipts.json) contains all
45 live/message case receipts, the private-bus sequence, and SHA-256 identities
for relevant production and test files. Win32 tuples are
`[message, wParam, client_x, client_y]`. GTK lines are `down/up button x y` or
`move modifier_mask x y`; GTK buttons are left=1, right=3, middle=2.
Native motion sampling can coalesce, so tests assert held motion and endpoint,
not a fixed number of intermediate events.

These are receiver/protocol observations, not semantic application drag/drop
success. `InputActionResult.verified` is unchanged. Windows physical-console and
other toolkit behavior are not established. Native release-failure injection,
shutdown cleanup and cancelled complete gestures were not added to the live
matrix; the existing shared coordinator regression tests cover those lifecycle
decisions. Force-killed processes remain outside the cleanup guarantee.

## Environment and commands

- Linux: `steam-deck-55d`, Debian 13.6, kernel `7.1.3+deb13-amd64`, GNOME Shell
  48.7, GTK 4.18.6, `wayland-0`, UID 1000. Logind reported Active=yes,
  LockedHint=no; GNOME ScreenSaver reported false. Existing `/dev/uinput` access
  and native development libraries were sufficient.
- Windows: `LUOLING-PC`, Windows 11 Pro `10.0.26100`; Kubernetes reported
  `10.0.26100.9457`. Existing Scoop-managed Rust 1.95.0 MSVC toolchain.
  SSH used `luoling8192` in service session 0; foreground receipt used the
  owner's existing `neko` RDP login in session 2 with a limited interactive token.
- Both native hosts used Cargo/Rust 1.95.0. Common driver unit tests: 51 passed
  on each host. Linux driver: 81 passed, 3 ignored before the new fixture;
  Windows driver: 82 passed, 1 ignored. All four isolated Portal tests, including
  the new held test and the three existing fixtures, subsequently passed.

Linux native tests:

```sh
cargo +1.95.0 test -p auv-driver-common -p auv-driver-linux --lib
cargo +1.95.0 test -p auv-driver-linux --lib native::portal::request::identity_tests -- --ignored --nocapture
env -u SWAYSOCK XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-0 \
  DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
  GDK_BACKEND=wayland XDG_CURRENT_DESKTOP=GNOME AUV_HELD_BACKEND=uinput \
  timeout 90s cargo +1.95.0 test -p auv-driver-linux --test held_mouse -- --ignored --nocapture
```

The GTK fixture requires the selected receiver to be active. Loss of activation
cancels the test's active input context. It is an opt-in test that opens a bounded
fullscreen window and moves the pointer. It does not restore the Linux pointer
because this path has no working current-position query. Select the actual
session's environment instead of copying these historical values blindly.

Windows native tests:

```sh
cargo test -p auv-driver-common -p auv-driver-windows --lib
cargo test -p auv-driver-windows --test held_mouse background_receives -- --ignored --nocapture
cargo test -p auv-driver-windows --test held_mouse foreground_receives -- --ignored --nocapture
```

The last command must run inside an unlocked interactive desktop. For this run,
the already-built executable was launched by a task-owned scheduled task using
the existing interactive token, not from SSH session 0. The receiver message
pump remained active while a separate worker invoked the public driver API.
Foreground ownership is checked during execution; lost ownership cancels the
active input context. On success the test restores the Windows pointer.

All native receiver runs exited 0. The final tests were rerun after adding
foreground-loss cancellation. Local `cargo fmt --check`, `git diff --check`,
and default `cargo test` passed. The default root CLI suite took 75 seconds;
`selected_global_text_dry_run_returns_validation_without_delivery` reported
running over 60 seconds but ultimately passed. No product fix was needed.

## Access, retained artifacts, and cleanup

The existing Cloudflare route was restarted and the owner completed browser
authentication. Kubernetes then reached both current node addresses through
`rc-dev/neko-dev`: Linux `10.0.0.196`, Windows `10.0.0.139`. Known IP host keys
were retained. No account, package, device permission, service configuration,
Portal selection, firewall or repository checkout on either host was changed.

Remote source and raw logs remain under:

- Linux: `/tmp/auv-bg1-native-20260923-dykeqj3r/`.
- Windows SSH user: `%TEMP%\auv-bg1-native-20260923-dykeqj3r\`.
- Windows interactive executable/logs:
  `C:\Users\Public\auv-bg1-native-20260923-dykeqj3r\`.

Linux logs are `native-tests.log`, `portal-held.log`, `uinput-held-final.log`.
Windows logs are `native-tests.log`, `background-held-final.log`, and
`foreground-held-final.log`, with `foreground-exit-final.txt` containing 0.
The checked-in receipt artifact preserves the meaningful event evidence even
if those temporary workspaces expire.

Task-owned receiver processes and the scheduled task were removed after testing.
The owner's RDP session was preserved. The task-owned loopback RDP forward remains
at `127.0.0.1:13389`; stop it after use with
`launchctl bootout gui/$(id -u)/dev.auv.bg1-native-20260923.rdp`.
Its configuration and local raw logs are in
`/var/folders/sx/gqsj9hgd5_n21t9tm8xw1x600000gn/T/auv-bg1-native-20260923-dykeqj3r/`.

The current Portal broker still lacks RemoteDesktop despite an unlocked GNOME
desktop. No shared service was restarted or reconfigured. Live Portal receipt
needs an approved coherent session whose selected backend exposes RemoteDesktop;
this remains an environment prerequisite, not a demonstrated held-input bug.
The other outstanding route in the held-input evidence matrix is macOS desktop,
which was outside this Linux/Windows validation request.
