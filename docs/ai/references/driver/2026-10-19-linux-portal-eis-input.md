# Linux Portal Input Through EIS

Status: implemented and live-validated for pointer, button, scroll, and keyboard
on GNOME Wayland, 2026-10-19. Held-button timing has a known common-layer
issue. See "Known Issue: Held-Button Deadline".

## Purpose

This note records the Linux portal input path after the EIS migration.

The public backend name is still `InputBackend::Portal`. The backend now sends
input through the EIS protocol. It does not use the RemoteDesktop `Notify*`
methods.

## Current Contract

The input path has these layers:

1. The RemoteDesktop portal owns permission, session lifetime, and restore
   tokens.
2. `RemoteDesktop.Start` selects keyboard and pointer devices.
3. `RemoteDesktop.ConnectToEIS` returns one end of an EIS socket.
4. The `reis` client performs the EIS handshake and owns the EIS devices.
5. `native/eis.rs` delivers pointer, button, scroll, and keyboard events.

The input session does not open ScreenCast. Input does not need a video stream.
The separate capture path still opens its own ScreenCast session when a caller
requests pixels.

If `ConnectToEIS` is not available, the input session fails. It does not fall
back to `Notify*` and it does not fall back to uinput.

## Session Flow

`InputSession::open` performs these steps:

1. Open the RemoteDesktop proxy on the caller connection.
2. Create a RemoteDesktop session.
3. Select `Keyboard` and `Pointer`.
4. Start the session with the persisted restore token when one exists.
5. Read the granted device set from the `Start` response.
6. Call `ConnectToEIS`.
7. Start the EIS worker and wait for the selected devices to resume.
8. Load the compositor XKB keymap from the EIS keyboard device.

The session closes the EIS worker before it closes the D-Bus session. This order
prevents late events on a closed portal session.

## Pointer Delivery

EIS exposes monitor viewports as regions. Each region has a logical position,
a logical size, and a scale.

AUV sends absolute motion in global logical coordinates. The EIS viewport
transform passes these coordinates through unchanged. The live test confirmed
that a target of `(1376, 576)` arrived at `(1376, 576)` on a 2752 by 1152
logical desktop with scale 1.25.

`EisInput::move_to` selects the first active absolute pointer device whose
region contains the target point. A point outside all regions is an invalid
input error. AUV does not send a point inside no region to a stale device.

The shared click path keeps its existing timing:

- 20 ms after absolute motion, before the button press.
- 34 ms between the button press and the button release.

Each transition uses one EIS frame. A press and its release use separate
frames.

## Buttons and Scroll

The button mapping is:

- Left: `BTN_LEFT` (`0x110`).
- Right: `BTN_RIGHT` (`0x111`).
- Middle: `BTN_MIDDLE` (`0x112`).

`EisInput::button` selects the absolute pointer by the supplied point for a
button press. It records that pointer device. A button release goes to the
same device, even when the pointer moved into another region during a drag.
This keeps the press and release route stable.

Absolute motion still selects the device by point. A cross-region drag can
therefore send motion through a different device than the held button. This
device split is not fully validated.

Scroll uses `ei_scroll.scroll_discrete`. One logical scroll notch maps to 120
discrete units. A positive `y` value scrolls down, which matches the existing
AUV scroll contract. Fractional pixels below one notch accumulate in the input
session.

## Keyboard

The EIS keyboard receives the compositor XKB keymap. AUV reads this keymap from
the EIS descriptor with positional reads. The local Wayland keymap is only a
fallback when the EIS keymap is absent.

The shared evdev mapping converts keysyms to evdev key codes and Shift needs.
This mapping is used by EIS and uinput.

The EIS worker tracks `KeyboardModifiers.group` and selects the matching XKB
layout. A US layout and a German layout can therefore keep different `y` and
`z` mappings. The local Wayland fallback has no group source. It keeps only
the mappings that agree in every layout. uinput keeps the same shared mapping.

A key chord is delivered in these steps:

1. Press the modifier keys in order.
2. Press the action key.
3. Release the action key.
4. Release the modifier keys in reverse order.

Each transition uses its own EIS frame. The live test confirmed these keyboard
cases:

- `a` produced `KeyA` down and up.
- `shift+a` produced `ShiftLeft` down, `KeyA` down, `KeyA` up, and
  `ShiftLeft` up.
- `type-text Hi!` produced the H, i, and exclamation key events.

## Worker and Lifecycle

`EiEventConverter` is not `Send`. One worker thread owns the EIS context and the
converter.

Callers send commands through a channel. A nonblocking wake pipe interrupts
`poll` when a command arrives. Each command has a synchronous reply channel.
The caller does not return until the worker has queued the EIS request and
flushed it. A flush failure returns a backend error and ends the worker.

The worker handles these events:

- Seat capability binding for absolute pointer, button, scroll, and keyboard.
- Device addition, removal, pause, and resume.
- Server disconnect.
- EIS ping and pong traffic through the converter.

A device starts in the paused state. The worker sends `ready` and
`start_emulating` after device creation. Commands only use resumed devices.

## Failure Behavior

These conditions return a backend error:

- Portal `ConnectToEIS` failure.
- EIS handshake failure or timeout.
- Missing selected device.
- Server disconnect.
- Closed EIS socket.
- EIS command flush failure.
- Keyboard or pointer use after pause.

The portal input session treats these errors as session failures. It does not
switch to another backend.

Invalid points, unsupported keysyms, and missing device capabilities return an
invalid input error. They do not replace the input session.

## Removed Surfaces

The migration removed these surfaces:

- All RemoteDesktop `Notify*` calls.
- The portal keysym keyboard path.
- The portal ScreenCast requirement for input.
- The display-to-stream output mapping for input.
- `ScreenCastStream::contains` and `ScreenCastStream::local_point` when no
  producer used them.

The public `InputBackend::Portal` and `InputBackend::Uinput` names did not
change.

## Ablation Results

The migration pass also removed code that had no remaining job:

- `PortalInput` was a pass-through type. Callers now use `InputSession::open`.
- The EIS worker had three separate shared fields. One `WorkerState` now holds
  the keymap, the failure text, and the connection flag.
- `chord_keys` returned a release list that no caller used. It now returns the
  press order only.
- The non-Linux input stub kept unused `scroll` and `key_press` methods. The
  migration removed them.
- The worker had a separate device wrapper that only forwarded calls. It is
  removed.

These parts remain because they carry behavior:

- The worker thread and wake pipe. `EiEventConverter` is not `Send`, and each
  command needs a prompt wakeup.
- The region check. It rejects points outside all EIS regions.
- The keymap mutex. The portal input path reads the keymap after the worker
  sets it.

## Live Evidence

Test host: GNOME Wayland, portal RemoteDesktop version 2.

Test receiver: a Chrome page that reported pointer, button, wheel, and key
events to a local HTTP server.

Observed results:

- `move 1376 576` reported a pointer event at `(1376, 576)`.
- `click 1376 576` reported pointer down and up at `(1376, 576)`.
- `scroll 1376 576 120` reported `deltaY=120`.
- `press a` reported `KeyA` down and up.
- `press shift+a` reported the expected Shift and A sequence.
- `type-text Hi!` reported the H, i, and exclamation key events.

The `held_mouse` integration test did not run on this host because the Python
`gi` module is absent. A temporary Rust probe used the same public API and the
Chrome receiver. It found the issue below.

## Known Issue: Held-Button Deadline

Live validation found an early button release for held input. The cause is in
`auv-driver-common/src/mouse_input.rs`, not in EIS.

`MouseCoordinator::down_admitted` calculates the hold deadline before it calls
`backend.move_to`. The first `move_to` opens the portal input session. That open
can take longer than the hold timeout. The watchdog then starts with no
remaining time and releases the button at once.

Live evidence:

- One 3-second hold released 0.4 milliseconds after the press. Session open
  used almost all 3 seconds.
- Other runs released between 0.4 and 1.5 seconds after the press.
- A normal click holds the button for the full 34-millisecond press interval.

The EIS button state is correct. The defect is the deadline start point in the
shared mouse coordinator.

The fix belongs in a follow-up branch after this EIS work merges. It must start
the deadline at the delivered button press and add a regression test. This
branch must not change the shared mouse coordinator.

Until the fix lands, the held mouse and drag live test is blocked.

## Validation Procedure

Automated tests cover a command flush failure.

Run these commands on a live GNOME Wayland desktop:

```text
cargo test -p auv-driver-linux
cargo run -p auv-driver-linux --example validate -- move 1376 576
cargo run -p auv-driver-linux --example validate -- click 1376 576
cargo run -p auv-driver-linux --example validate -- scroll 1376 576 120
cargo run -p auv-driver-linux --example validate -- press a
cargo run -p auv-driver-linux --example validate -- press shift+a
cargo run -p auv-driver-linux --example validate -- type-text Hi!
```

Make sure that a receiver window covers the target point. Before you send
keyboard input, make sure that the receiver has focus.

## Deferred Work

These items are outside this slice:

- Relative pointer delivery. The current backend binds only
  `PointerAbsolute`.
- Live multi-monitor drag validation. The code keeps a button release on the
  press device, but this host has one output. The motion route during a
  cross-region drag also needs a multi-monitor test.
- High-resolution continuous scrolling. The current contract uses discrete
  notches.
- A focused fake EIS input server test. Current tests cover the handshake and
  a command flush failure, but not full device input.
- uinput layout-group tracking. uinput still uses only mappings that agree in
  every layout. EIS uses the compositor group event.
- Live multi-layout keyboard validation. Unit tests cover US and German
  groups. The live check used one layout.
- The held-button deadline fix in `auv-driver-common`. See "Known Issue:
  Held-Button Deadline".
