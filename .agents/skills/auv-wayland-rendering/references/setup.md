# Setup recipes

Use this reference to select dependencies and session topology. Prefer the
host's existing Wayland desktop. Use a headless compositor only when no desktop
exists or isolation is required.

## Capability map

| Capability | Typical component | Required for AUV capture |
| --- | --- | --- |
| Wayland compositor | Sway or the host desktop | Yes |
| Portal frontend | `xdg-desktop-portal` | Yes |
| wlroots portal backend | `xdg-desktop-portal-wlr` | For Sway/wlroots |
| Frame transport | PipeWire | Yes for ScreenCast |
| Session policy | WirePlumber | Normally yes |
| Output inspection | `wayland-info`, compositor IPC | Diagnostic |
| Independent screenshot | `grim` | Diagnostic on wlroots |
| Remote human view | WayVNC | Optional on wlroots |
| Test client | Foot or another Wayland app | Recommended |

Do not install every portal backend. Match the backend to the compositor and
inspect the current portal selection before changing it.

## Distribution examples

Package names change. Confirm them with the distribution package index before
installing.

Debian or Ubuntu family:

```bash
sudo apt install \
  sway xdg-desktop-portal xdg-desktop-portal-wlr \
  pipewire wireplumber wayland-utils grim foot
sudo apt install wayvnc  # optional
```

Arch Linux family:

```bash
sudo pacman -S \
  sway xdg-desktop-portal xdg-desktop-portal-wlr \
  pipewire wireplumber wayland-utils grim foot
sudo pacman -S wayvnc  # optional
```

For AUV build dependencies, prefer the repository's `nix develop` shell. If
using native packages, derive the current library set from `flake.nix` and CI
rather than copying an old list.

## Existing desktop session

Run AUV inside the session that already owns the Wayland socket and D-Bus. Over
SSH, recover the compositor variables only if the desktop imported them into
the systemd user manager:

```bash
.agents/skills/auv-wayland-rendering/scripts/with-wayland-session.sh \
  target/debug/auv invoke display.capture
```

If the helper cannot find a live socket, publish the variables from a process
started by the compositor. For Sway, a configuration command can do this:

```text
exec systemctl --user import-environment WAYLAND_DISPLAY SWAYSOCK XDG_CURRENT_DESKTOP
```

Restart the selected portal services after publishing a changed compositor
environment. Avoid doing this when another live desktop session depends on the
same user services.

## Headless Sway session

Use a user service or another supervisor so the compositor and its children
share a lifetime. Start with:

```text
XDG_CURRENT_DESKTOP=sway
XDG_SESSION_TYPE=wayland
WLR_BACKENDS=headless
WLR_HEADLESS_OUTPUTS=1
WLR_LIBINPUT_NO_DEVICES=1
```

Do not set `WLR_RENDERER` for the first attempt. Inspect Sway's debug or journal
logs to learn what wlroots selected.

To require the GLES2 path, set `WLR_RENDERER=gles2` and do not set
`WLR_RENDERER_ALLOW_SOFTWARE=1`. Treat a failure as missing GPU/driver/device
support, not as permission to silently relabel software rendering as GPU.

For a deterministic software fallback, use:

```text
WLR_RENDERER=pixman
```

The fallback is useful for capture correctness tests but is not GPU evidence.

A minimal Sway configuration can define the output and publish the generated
session variables:

```text
output HEADLESS-1 mode 1280x720
output * bg #1f2430 solid_color
exec sh -lc 'systemctl --user import-environment WAYLAND_DISPLAY SWAYSOCK XDG_CURRENT_DESKTOP XDG_SESSION_TYPE && systemctl --user restart xdg-desktop-portal-wlr.service xdg-desktop-portal.service'
exec foot --title AUV-Wayland-Validation
```

Confirm the real output name with Sway IPC before putting it in portal
configuration.

For unattended wlroots capture, use an xdpw configuration such as:

```ini
[screencast]
output_name=HEADLESS-1
max_fps=30
chooser_type=none
```

`chooser_type=none` bypasses interactive consent for the selected output. Use
it only in an isolated session whose owner requested unattended capture.

## GPU access

Check all layers instead of stopping at PCI discovery:

1. The host driver binds the device and exposes `/dev/dri/renderD*`.
2. The session user can open the chosen render node.
3. A container, if used, receives the render node and matching user/group IDs.
4. EGL or Vulkan reports the intended device and driver.
5. The compositor log reports the hardware renderer rather than `pixman`,
   `llvmpipe`, or another software implementation.
6. Portal capture succeeds without an implicit software-only claim.

For NVIDIA, AMD, Intel, virtual GPUs, and containers, follow the compositor and
driver documentation for that exact stack. Do not encode vendor workarounds as
universal defaults.

## Remote viewing

Remote viewing is optional and separate from AUV capture. For wlroots, keep
WayVNC on loopback:

```bash
wayvnc 127.0.0.1 5900
```

Forward it from the client:

```bash
ssh -N -L 5900:127.0.0.1:5900 user@host
```

Then connect a VNC viewer to `127.0.0.1:5900`. If loopback plus SSH tunneling
is unsuitable, configure WayVNC authentication and TLS before changing the
listen address.

## Primary upstream references

- [wlroots environment variables](https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/master/docs/env_vars.md)
- [xdg-desktop-portal-wlr configuration](https://github.com/emersion/xdg-desktop-portal-wlr/blob/master/xdg-desktop-portal-wlr.5.scd)
- [xdg-desktop-portal-wlr portal selection](https://github.com/emersion/xdg-desktop-portal-wlr)
- [WayVNC remote-access guidance](https://github.com/any1/wayvnc/blob/master/README.md)
