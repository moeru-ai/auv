---
name: auv-wayland-rendering
description: Prepare, diagnose, and validate Linux GPU and Wayland environments for AUV rendering and capture through XDG Desktop Portal and PipeWire. Use when installing AUV desktop runtime dependencies, connecting from another machine over SSH, using an existing compositor or a headless wlroots/Sway session, checking hardware versus software rendering, running display.capture or Linux driver probes, exposing optional WayVNC safely, or troubleshooting blank frames and Wayland, portal, D-Bus, PipeWire, or session-environment failures.
---

# AUV Wayland Rendering

Build a reproducible Linux desktop session in which AUV can observe a Wayland
display and persist capture artifacts. Treat environment validation as evidence,
not as a change to AUV's public support claim.

Keep ownership explicit: the compositor renders application clients; AUV
requests frames through the portal and records them. Do not imply that AUV
ships the compositor or a Linux overlay renderer.

## Read the relevant references

- Read [setup.md](references/setup.md) before installing packages, creating a
  compositor session, or configuring remote viewing.
- Read [troubleshooting.md](references/troubleshooting.md) when a probe or
  capture fails, the image is blank, or hardware rendering is uncertain.
- Check AUV's current [support matrix](../../../docs/SUPPORT_MATRIX.md) before
  describing a result as supported. A successful live probe is
  `live-validated` evidence for the named environment only.

## Follow the workflow

### 1. Inspect before changing the host

Run the read-only probe in the target shell:

```bash
.agents/skills/auv-wayland-rendering/scripts/probe-wayland.sh
```

Add `--require-gpu` to make missing GPU prerequisites fatal when hardware
rendering is required. The flag does not replace compositor-log evidence.
Record:

- distribution and package manager;
- existing compositor and whether the session is local, nested, or headless;
- `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`, and the session D-Bus;
- accessible `/dev/dri/renderD*` nodes and the renderer reported by compositor
  logs or `eglinfo`/`vulkaninfo`;
- ScreenCast portal and PipeWire readiness.

Do not infer GPU rendering from the presence of a GPU or render node. Require a
renderer log or renderer-tool result. Label `pixman`, `llvmpipe`, and similar
paths as software rendering.

### 2. Choose the session shape

Prefer the existing logged-in Wayland compositor when one is available. It
already owns the seat, GPU, D-Bus activation environment, and user consent UI.

Use headless Sway when the host has no desktop session or the test must be
isolated. Start with wlroots renderer auto-selection. Force `pixman` only as an
explicit software fallback; doing so gives deterministic rendering but does
not validate the GPU path.

Use a nested compositor when isolation is needed inside an existing Wayland
desktop and an extra window is acceptable. Do not point AUV at one compositor
while its portal backend is attached to another.

### 3. Install the smallest capability set

Install or verify:

- a Wayland compositor such as Sway;
- `xdg-desktop-portal` and the backend for that compositor;
- PipeWire and WirePlumber;
- `wayland-info` and `grim` for independent diagnostics;
- WayVNC only when a human needs to view the wlroots session remotely.

Request authorization before changing system packages, user services, group
membership, login linger, or firewall state. Prefer distribution packages.
Use a source build or patch only after reproducing a version-specific defect
and recording the exact upstream revision and reason.

### 4. Keep one coherent user session

Run the compositor, portal backend, PipeWire, and AUV as the same unprivileged
user. They must agree on `XDG_RUNTIME_DIR`, `WAYLAND_DISPLAY`, and the session
bus. For a compositor started by a user service, import its generated variables
into the systemd user manager and retrieve them in SSH shells.

Use the helper to run a command with either the current environment or the
variables published by the user manager:

```bash
.agents/skills/auv-wayland-rendering/scripts/with-wayland-session.sh \
  auv invoke display.list --json
```

Do not run the compositor or AUV as root to work around device permissions.
Fix seat, logind, container-device, or render-node access instead.

### 5. Validate from dependencies to AUV artifacts

Validate in this order so each failure has one clear owner:

1. Confirm the Wayland socket and compositor output with `wayland-info` or the
   compositor IPC.
2. Confirm PipeWire and the `org.freedesktop.portal.ScreenCast` interface.
3. Run the AUV Linux driver probe:

   ```bash
   cargo run -p auv-driver-linux --example validate -- permissions displays
   ```

4. Run repeated product-surface captures:

   ```bash
   .agents/skills/auv-wayland-rendering/scripts/verify-auv-capture.sh \
     --auv target/debug/auv --repeat 3
   ```

5. Open at least one emitted PNG. Verify that it contains the expected client,
   has the expected dimensions, and is not a blank or stale frame.

Repeated captures establish startup and first-frame reliability. To test
freshness, visibly change a client between two capture passes and compare their
artifacts; identical static frames do not prove that updates propagate.

The verification script checks AUV's JSON result, artifact purpose, PNG
signature, and file existence. It does not prove semantic correctness of the
rendered application; visual inspection or an app-specific assertion remains
separate.

### 6. Report the boundary precisely

Report all of the following:

- OS, compositor, portal backend, output name and dimensions;
- renderer and whether it is hardware or software;
- portal frame transport when known (`dma-buf`, shared memory, or fallback);
- AUV revision and exact validation commands;
- successful run IDs and artifact paths;
- whether remote viewing was enabled and how it was secured;
- known warnings, fallbacks, and missing capabilities.

Do not turn compilation, a portal introspection result, or a single screenshot
into a blanket Linux support claim.

Keep GPU compositor rendering, GPU-buffer transport, and correct AUV artifact
output as three separate claims. None implies the other two.

## Apply safety rules

- Bind WayVNC to loopback and use an SSH tunnel unless authenticated transport
  was explicitly configured.
- Do not expose an unauthenticated VNC port on `0.0.0.0`.
- Do not overwrite an existing desktop's portal selection without checking its
  current backend and other consumers.
- Do not persist services or enable login linger unless the user requested a
  durable session.
- Keep capture and input evidence separate. A rendered frame does not prove
  that input was delivered or that an application completed an operation.
