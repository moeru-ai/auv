# Linux uinput and rc Wayland validation

Owner-approved slice (2026-09-12): add a Linux-only evdev/uinput input backend
and verify that an ihome rc Workspace can render a Wayland application.

- Keep Portal as the default. Select uinput explicitly with a typed driver
  option or `AUV_LINUX_INPUT_BACKEND=uinput` in the first-party CLI/daemon host.
- Select once when opening an input session; never retry an uncertain delivered
  event through another backend.
- Reuse ClickModifiers and InputActionResult. Input remains foreground system
  input and does not establish semantic success.
- Read the compositor's XKB map for keysym-to-evdev conversion rather than
  assuming a US keyboard. Map unshifted/Shift levels and retain only strokes shared by every
  configured layout; unsupported mappings fail before sending a chord.
- Create named AUV virtual devices through /dev/uinput only. No real keyboard
  event devices are opened for reading.
- Keep image capture independent. rc's headless Sway image supports software
  rendering and unattended ScreenCast, but headless-only Sway does not consume
  kernel uinput devices. A rendered container does not prove injected input.

## Evidence

- Linux driver compilation and 73 tests pass on Debian `neko-gpu-1`, including
  US/German XKB mapping, shared-layout agreement, repeated keymap descriptor
  reads, key release order, pointer coordinates, and Portal D-Bus fixtures.
- After graphical login, GNOME Wayland (`wayland-0`, DP-3, 2560x1440) delivered
  `a`, `Shift+B`, Ctrl+Shift-click and scrolling into a dedicated GTK window.
  Five consecutive final clicks carried both modifiers at button press; releases
  arrived in reverse order and the subsequent scroll had no Ctrl/Shift bits.
  Four of these clicks used real CLI invocations with repeated `--modifiers`.
  CLI run `01a09205-674b-7912-a3d3-019948c36365` completed and recorded
  `foreground_system_events`, `verified: false` as expected.
- [GTK event receipt](evidence/2026-09-12-uinput/events.jsonl),
  [test window](evidence/2026-09-12-uinput/scene.py), and
  [driver probe](evidence/2026-09-12-uinput/probe.rs) preserve reproduction inputs.
  The receipt also includes failed intermediate ordering attempts. GTK Wayland
  coordinates are surface-relative (the requested screen y=400 appeared as
  y=367 below the desktop's top region); they are not a global-coordinate oracle.
- Live testing found two defects fixed before publication: shared keymap FD
  offsets broke subsequent connections, and Mutter dispatched button presses
  before pending keyboard modifiers. Positional FD reads fix the first; one
  virtual device plus a 40ms modifier propagation interval passed the latter
  probe. The interval is a tested workaround, not a compositor acknowledgement
  or a guarantee under arbitrary load.
- Windows `cargo tree -p auv-cli --target x86_64-pc-windows-msvc -i evdev`
  reports no dependency. evdev/xkbcommon are Linux target dependencies only.
- ihome context `neko-mbp14@kubernetes`, namespace `rc-dev`, Workspace
  `auv-wayland-validation`, on node `neko-gpu-1`, image
  `ghcr.io/nekomeowww/rc/runner-wayland:0.11.0`, persistent 20Gi `tns-iscsi` home.
  Other Workspaces and their processes were left unchanged.
- rc Wayland health passed. Sway renders `HEADLESS-1` at 1280x720 using pixman;
  there is no allocated GPU/render node. Portal logs show `wl_shm` transport.
- Electron 44.0.0 rendered a test window. The image's AUV 0.0.13 captured three
  frames through `xdg-desktop-portal.screencast.pipewire`, without fallback or
  interactive consent. This verifies the rc image, not the new input code.
- Captures `01a091f1-d969-79d0-981a-209f990ee422` and
  `01a091f2-cd21-7341-a85e-1360b630068c` show different clock text and hashes;
  a third immediate capture `01a091f2-cd70-7d13-9d65-4f659e10fff9` also completed.
  [First frame](evidence/2026-09-12-wayland/first.png) and
  [later frame](evidence/2026-09-12-wayland/later.png) preserve the visible output.
- No VNC, ingress, or public endpoint was created. The Workspace and its
  supervised test app remain available for continued validation.

## Operating commands

All rc commands use `KUBECONFIG="$HOME/.kube/config.d/ihome.conf"`.

```sh
rcctl -n rc-dev agent exec --workspace auv-wayland-validation --no-env-passthrough -- rc-wayland-health
rcctl -n rc-dev agent exec --workspace auv-wayland-validation --no-env-passthrough -- auv invoke display.capture --store-root /home/agent/auv-runs --json
rcctl -n rc-dev agent logs process-mtxclkjwd577205f
rcctl -n rc-dev workspace stop auv-wayland-validation
```

Start the Workspace again with `workspace start`. Its home, installed Electron,
`/home/agent/auv-validation/scene.cjs`, and artifacts persist. Relaunch the test
app as a new supervised AgentProcess:

```sh
rcctl -n rc-dev agent run --detach --workspace auv-wayland-validation --no-env-passthrough --cwd /home/agent/auv-validation -- ./node_modules/.bin/electron --no-sandbox --ozone-platform=wayland --use-gl=angle --use-angle=gl --ignore-gpu-blocklist --enable-gpu-rasterization scene.cjs
```

The image emits nonfatal GPU/Vulkan and missing optional Portal errors under
software rendering. The application frame, not GPU-process existence, is the
rendering evidence.

## Selecting uinput

Library hosts select `LinuxDriver::with_input_backend(InputBackend::Uinput)` or
`LocalDriver::with_linux_input_backend(LinuxInputBackend::Uinput)`. For direct
CLI and daemon-owned local Runners, set `AUV_LINUX_INPUT_BACKEND=uinput` before
starting AUV. The default remains `portal`; invalid values return a configuration
error. Restart the daemon to change its inherited configuration.

The initial uinput backend requires one output for absolute pointer mapping and
agreement across configured XKB layouts for each requested key. Mapping supports unshifted/Shift levels; AltGr/compose,
IME, lock-state-aware text, and multiple-layout tracking remain deferred.
Clipboard and screenshots retain their existing independent backends. Access to
`/dev/uinput` is required; AUV does not modify host device permissions itself.


Local macOS `cargo check` and the default `cargo test` suite pass. Linux Clippy
reports only the existing AT-SPI/session/window-test warnings. Live delivery
evidence is specific to the GNOME session above.

## Existing keyboard frontend boundary

Linux driver keyboard delivery is live-validated through its typed `InputApi`.
The higher-level `input.key`, `input.keys`, and `input.keyboard` invoke commands
still use the existing macOS-only batch keyboard contract. Selecting uinput does
not enable those commands. Connecting that batch contract on Linux remains a
separate slice; pointer CLI and existing local Runner input routes use the new
backend through the shared driver factory.
