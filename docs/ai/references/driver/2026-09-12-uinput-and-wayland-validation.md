# Linux uinput and rc Wayland validation

Owner-approved slice (2026-09-12): add a Linux-only evdev/uinput input backend
and obtain evidence that an ihome rc Workspace can render a Wayland application.

- Keep Portal as the default. Select uinput explicitly with a typed driver
  configuration or `AUV_LINUX_INPUT_BACKEND=uinput` in the first-party CLI/daemon host.
- Before input starts, select the backend once for the session. Never retry an uncertain delivered
  event through another backend.
- Reuse ClickModifiers and InputActionResult.

  Input remains foreground system input and does not establish semantic success.
- Read the compositor's XKB map for keysym-to-evdev conversion rather than
  assuming a US keyboard.
  Map unshifted/Shift levels. Retain only strokes shared by every configured layout.

  Unsupported mappings fail before chord delivery.
- Create named AUV virtual devices through /dev/uinput only.

  The driver does not read real keyboard event devices.
- Keep image capture independent.

  The rc headless Sway image supports software
  rendering and unattended ScreenCast, but headless-only Sway does not consume
  kernel uinput devices. A rendered container does not prove injected input.

## Evidence

- Linux driver compilation and 73 tests pass on Debian `neko-gpu-1`.
  Tests cover US/German XKB mapping, shared-layout agreement, and repeated keymap descriptor reads.
  Other tests cover key release order, pointer coordinates, and Portal D-Bus fixtures.
- After graphical login, GNOME Wayland (`wayland-0`, DP-3, 2560x1440) delivered
  `a`, `Shift+B`, Ctrl+Shift-click and scrolling into a dedicated GTK window.
  Five consecutive final clicks carried both modifiers at button press. Releases
  arrived in reverse order and the subsequent scroll had no Ctrl/Shift bits.
  Four of these clicks used real CLI invocations with repeated `--modifiers`.
  CLI run `01a09205-674b-7912-a3d3-019948c36365` completed and recorded
  `foreground_system_events`, `verified: false` as expected.
- [GTK event receipt](evidence/2026-09-12-uinput/events.jsonl),
  [test window](evidence/2026-09-12-uinput/scene.py), and
  [driver probe](evidence/2026-09-12-uinput/probe.rs) preserve reproduction inputs.
  The receipt also includes failed intermediate ordering attempts. GTK Wayland
  coordinates are surface-relative (the requested screen y=400 appeared as
  y=367 below the desktop's top region). They are not a global-coordinate oracle.
- Live testing found two defects fixed before publication: shared keymap FD
  offsets broke subsequent connections, and Mutter dispatched button presses
  before pending keyboard modifiers. Positional FD reads fix the first. One
  virtual device plus a 40ms modifier propagation interval passed the latter
  probe. The interval is a tested workaround, not a compositor acknowledgement
  or a guarantee under arbitrary load.
- Windows `cargo tree -p auv-cli --target x86_64-pc-windows-msvc -i evdev`
  reports no dependency. evdev/xkbcommon are Linux target dependencies only.
- ihome context `neko-mbp14@kubernetes`, namespace `rc-dev`, Workspace
  `auv-wayland-validation`, on node `neko-gpu-1`, image
  `ghcr.io/nekomeowww/rc/runner-wayland:0.11.0`, persistent 20Gi `tns-iscsi` home.
  The experiment left other Workspaces and their processes unchanged.
- rc Wayland health passed. Sway renders `HEADLESS-1` at 1280x720 using pixman. There is no allocated GPU/render node. Portal logs show `wl_shm` transport.
- Electron 44.0.0 rendered a test window. The image's AUV 0.0.13 captured three
  frames through `xdg-desktop-portal.screencast.pipewire`, without fallback or
  interactive consent. This evidence applies to the rc image. It does not establish the behavior of the new input code.
- Captures `01a091f1-d969-79d0-981a-209f990ee422` and
  `01a091f2-cd21-7341-a85e-1360b630068c` show different clock text and hashes. A third immediate capture `01a091f2-cd70-7d13-9d65-4f659e10fff9` also completed.
  [First frame](evidence/2026-09-12-wayland/first.png) and
  [later frame](evidence/2026-09-12-wayland/later.png) preserve the visible output.
- The experiment created no VNC, ingress, or public endpoint. The Workspace and its
  supervised test app remain available for continued validation.

## Operating commands

All rc commands use `KUBECONFIG="$HOME/.kube/config.d/ihome.conf"`.

```sh
rcctl -n rc-dev agent exec --workspace auv-wayland-validation --no-env-passthrough -- rc-wayland-health
rcctl -n rc-dev agent exec --workspace auv-wayland-validation --no-env-passthrough -- auv invoke display.capture --store-root /home/agent/auv-runs --json
rcctl -n rc-dev agent logs process-mtxclkjwd577205f
rcctl -n rc-dev workspace stop auv-wayland-validation
```

Start the Workspace again with `workspace start`.

Its home, installed Electron, `/home/agent/auv-validation/scene.cjs`, and artifacts persist.

Relaunch the test app as a new supervised AgentProcess:

```sh
rcctl -n rc-dev agent run --detach --workspace auv-wayland-validation --no-env-passthrough --cwd /home/agent/auv-validation -- ./node_modules/.bin/electron --no-sandbox --ozone-platform=wayland --use-gl=angle --use-angle=gl --ignore-gpu-blocklist --enable-gpu-rasterization scene.cjs
```

The image emits nonfatal GPU/Vulkan and missing optional Portal errors under
software rendering. The application frame, not GPU-process existence, is the
rendering evidence.

## Selecting uinput

Library hosts select `LinuxDriver::with_input_backend(InputBackend::Uinput)` or
`LocalDriver::with_linux_input_backend(LinuxInputBackend::Uinput)`.
The default remains `portal`. Invalid values return a configuration error.

For direct CLI and daemon-owned local Runners, set `AUV_LINUX_INPUT_BACKEND=uinput` before AUV starts.
Restart the daemon to change its inherited configuration.

The initial uinput backend requires one output for absolute pointer mapping and
agreement across configured XKB layouts for each requested key. Mapping supports unshifted/Shift levels. AltGr/compose,
IME, lock-state-aware text, and multiple-layout tracking remain deferred.
Clipboard and screenshots retain their existing independent backends.
The driver requires access to `/dev/uinput`. AUV does not modify host device permissions itself.

Local macOS `cargo check` and the default `cargo test` suite pass. Linux Clippy
reports only the existing AT-SPI/session/window-test warnings. Live delivery
evidence is specific to the GNOME session above.

## Keyboard frontend integration

The Linux foreground batch contract now connects `input.key`, `input.keys`,
`input.typeText`, `input.pasteText`, and `input.keyboard` to
`InputApi::input_keyboard`. Direct invoke and local Runner RPCs share validation,
ordered delivery, repetitions, and `KeyboardInputProgress` errors. Existing
Runner clients and Proto messages already carry this contract. No schema or SDK
payload changes are needed. Legacy shortcut parsing reuses list-key validation.

Validation rejects empty/invalid combinations, misplaced or duplicate modifiers,
invalid repetition counts/intervals, unsupported text, and unsupported targets
before delivering the batch prefix. uinput also makes sure that all required key mappings exist before delivery. Dry-run performs structural and policy validation without
creating a backend session. It does not establish live layout or permission
readiness. Delivery failures retain completed actions and completed repetitions.

Live GNOME verification through the actual CLI produced `aBBcd!` from a single
key, a repeated Shift+B combination, and a press-plus-text batch. An invalid
second action rejected its `x` prefix without sending it. A selected Device
invocation then traversed daemon -> Runner -> InputKeyboard RPC and appended `R`
with Shift+R, Run `a97589da-8173-bb6d-a4bc-169d69b1e2b1`.
[CLI receipt](evidence/2026-09-12-keyboard/cli.txt),
[Runner receipt](evidence/2026-09-12-keyboard/runner.txt), and
[CLI reproduction](evidence/2026-09-12-keyboard/probe.py) preserve this evidence.
The test uses the dedicated GTK scene linked above.

Linux application/window-targeted batches remain unsupported until recipient
preparation and identity validation exist. They never silently become global
input. Text retains the existing ASCII limit. Clipboard paste uses its existing
Portal authorization path and was not live-tested in this validation. Windows
batch input remains an explicit unsupported capability. This slice connects
Linux and preserves the existing macOS implementation.

Regression coverage includes Linux driver preflight and dry-run tests, invoke
command dispatch, and Runner RPC error-progress decoding. Linux's 75 driver tests
(including Portal fixtures), both new frontend tests, macOS frontend suites,
`cargo check -p auv-cli`, formatting, and full `pnpm lint` pass.

## September 13 review follow-up

The review keeps the existing public contracts and backend selection. Private
keyboard plans now define the chords and delays used by both preflight and
execution, including replace/submit. A batch retains one XKB snapshot through
all actions and repetitions, including clipboard operations. Direct operations
prepare their own snapshot.

Plain clicks do not read XKB. The next operation reads layout changes. The active operation does not track these changes live. This does not
provide focus isolation or semantic verification.

The virtual device's advertised key capabilities remain fixed at creation.
The driver compares updated layouts with these capabilities, including synthesized Shift.
It rejects a missing code before delivery. A new driver session is necessary to use that code.

InvalidInput failures preserve healthy input sessions. Backend/permission failures
still discard the session. The driver never automatically replays failed input.

Portal keyboard chords now share held-key cleanup with modifier clicks. After failed replies, the driver attempts releases in reverse order for all attempted keys.
Release errors remain visible. A private D-Bus receiver reproduced the missing releases and discarded sessions before the fix.
After the fix, it recorded cleanup and a valid click after a rejected coordinate on the same session.

Clipboard pipe transfers now use the existing async-io dependency to make the
received descriptors nonblocking and enforce the existing two-second deadline
across the complete read/write. Tests with stalled blocking pipes reproduced the
previous unbounded waits. A UTF-8 payload larger than pipe capacity also completes.
This bounds FD transfer waits during worker teardown, not every possible Portal
service shutdown delay.

Local Runner routing uses its class-level creation lock without acquiring a
second per-Run creation lock. Affinity records and admission synchronization
against StopRun remain intact. Custom providers retain their per-affinity locks.

Live GNOME CLI verification repeated `a`, Shift+B twice, and a press/text batch.
The dedicated GTK receiver observed `aBBcd!`. An invalid tail did not emit `x`.
[Receiver events](evidence/2026-09-13-input-review/gtk.jsonl) preserve this check.
This does not add a live clipboard or mid-operation layout-switch claim.

Follow-up validation: Linux driver suite passed 82 tests including ignored
private-bus fixtures. Linux Clippy completed with only the existing AT-SPI and
session/window-test warnings. macOS driver stub suite passed 38 tests, daemon
suite passed 22, and both CLI frontend library suites passed. SDK daemon tests
passed eight with the Windows-specific test skipped on macOS. Changed-crate
formatting and `git diff --check` passed.

Three sub-agents reviewed Linux input,
daemon/SDK lifecycle, and Portal teardown. No unresolved blocker remained.
