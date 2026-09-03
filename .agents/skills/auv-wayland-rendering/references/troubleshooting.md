# Troubleshooting

Diagnose one boundary at a time. Preserve the failing command, exit status,
relevant environment, and service logs before changing configuration.

## Failure map

| Symptom | Likely boundary | Check first |
| --- | --- | --- |
| `WAYLAND_DISPLAY` unset over SSH | Session environment | `systemctl --user show-environment` and the live socket |
| Wayland socket exists but clients fail | User/runtime mismatch | socket owner, `XDG_RUNTIME_DIR`, client UID |
| Portal name missing on D-Bus | Portal frontend | `xdg-desktop-portal` user service and session bus |
| ScreenCast interface missing | Wrong portal backend selection | `*-portals.conf`, compositor name, xdpw service |
| Portal can select an output but no frame arrives | screencopy/buffer negotiation | xdpw and compositor versions, DMA-BUF versus SHM logs |
| PipeWire remote or node fails | PipeWire session | PipeWire/WirePlumber services and runtime socket |
| Capture is black or stale | Renderer or screencopy path | compositor renderer logs and an independent `grim` capture |
| Headless compositor starts only with `pixman` | GPU initialization | render-node access, EGL/Vulkan driver, compositor logs |
| WayVNC reports no output | Wrong compositor environment | `WAYLAND_DISPLAY`, `WAYVNC_OUTPUT`, compositor IPC |
| First capture fails but later captures work | Initial-frame protocol defect | reproduce several times and test a maintained xdpw release |

## Session environment

Compare the compositor, portal backend, and AUV processes:

```bash
tr '\0' '\n' </proc/COMPOSITOR_PID/environ | \
  grep -E '^(XDG_RUNTIME_DIR|WAYLAND_DISPLAY|SWAYSOCK|DBUS_SESSION_BUS_ADDRESS|XDG_CURRENT_DESKTOP)='
```

Repeat for the portal process. Do not copy environment values blindly: stale
Wayland and Sway sockets can remain named after an earlier session. Verify that
the current process can use them.

## Portal selection and introspection

Check the public portal interface:

```bash
busctl --user introspect \
  org.freedesktop.portal.Desktop \
  /org/freedesktop/portal/desktop \
  org.freedesktop.portal.ScreenCast
```

For Sway, confirm that the preferred portal configuration selects `wlr` for
ScreenCast. A generic portal frontend being active does not prove that the
correct backend owns the implementation.

Inspect logs together:

```bash
journalctl --user \
  -u xdg-desktop-portal.service \
  -u xdg-desktop-portal-wlr.service \
  -u pipewire.service \
  -u wireplumber.service \
  --since '10 minutes ago'
```

## No valid screencopy format

If xdpw selects the output but reports that it cannot receive a valid format
from wlroots screencopy:

1. Capture the complete xdpw and compositor versions.
2. Check whether the compositor advertises DMA-BUF, SHM, or both.
3. Reproduce with the current maintained distribution package or upstream
   release.
4. Compare an independent `grim` capture.
5. Use a source patch only when an upstream issue or exact code inspection
   demonstrates the missing protocol path.

Do not distribute a locally copied binary or undocumented patch. Record source
URL, revision, checksum, build recipe, patch, and the failing/passing probes.

## GPU versus software rendering

Use at least two compatible evidence points when possible:

- compositor debug or journal renderer initialization;
- `eglinfo -B` or `vulkaninfo --summary`;
- render-node file descriptor or device identity;
- vendor monitoring tools while a changing scene renders.

The following are not sufficient by themselves:

- `lspci` or a vendor utility listing the GPU;
- `/dev/dri/renderD*` existing;
- Sway running with `WLR_BACKENDS=headless`;
- a successful PNG capture.

If `pixman` was forced, report software rendering even when a GPU is present.

## AUV capture diagnosis

Run the driver probe before the product command:

```bash
cargo run -p auv-driver-linux --example validate -- permissions displays capture-screen
```

Then run:

```bash
target/debug/auv invoke display.capture --json
```

Interpret the layers separately:

- Build success proves compilation only.
- `permissions` proves AUV can inspect the session and portal interfaces.
- `displays` proves output discovery and geometry.
- `capture-screen` exercises the Linux driver directly.
- `display.capture` proves the product invoke path and durable artifact
  projection.
- Opening the PNG proves that the expected visual content arrived.

When fallback occurs, preserve AUV's `fallback_reason`. A successful fallback
must not be reported as success for the primary backend.

## Containers

Treat a container as another session boundary. Confirm:

- the matching Wayland socket is mounted;
- `XDG_RUNTIME_DIR` inside the container resolves that mount;
- the session D-Bus and PipeWire sockets are reachable;
- the render device is passed through with usable ownership;
- host and container graphics libraries are compatible.

Prefer a compositor and portal entirely inside the container or entirely on
the host. Splitting ownership across both requires deliberate socket and D-Bus
plumbing and should be documented as a distinct topology.
