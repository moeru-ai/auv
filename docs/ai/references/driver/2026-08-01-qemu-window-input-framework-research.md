# QEMU window input delivery: AUV, CUA, and KWWK

Date: 2026-08-01

## Question

Why can AUV report a successful macOS window-targeted click against Android
Emulator while the Android guest receives no tap, and how do comparable
computer-use frameworks handle this boundary?

## Conclusion

The reproduced failure is not a coordinate or OCR failure. macOS accepted the
request to post a mouse event to the QEMU process, but that does not prove that
the emulator's host UI forwarded a complete click into the guest. QEMU-style
display frontends commonly gate guest input on window focus or mouse-grab
state.

AUV at the reproduced base revision treated a successful event-posting call as
a successful attempt. For click operations, `ForegroundPreferred` tried the background
`WindowTargetedMouse` path first and only falls back when that path returns an
error. A silent no-op therefore prevents the foreground rung from running.

The most transferable patterns from comparable frameworks are:

1. Distinguish event delivery from semantic verification.
2. Make background and foreground delivery explicit ladder rungs.
3. Verify the effect after an unverifiable click before escalating.
4. For Android, prefer a guest-aware transport such as ADB over synthetic input
   aimed at the host QEMU window.

## Local reproduction

The test used the `auv` binary built from local `main` in an isolated worktree.

```text
window: Android Emulator - Pixel_10:5554
process: qemu-system-aarch64
pid: 16296
window id: 545200
```

Before and after this command, Android UIAutomator reported `tap 4`:

```sh
auv invoke window.clickText 'Native button' \
  --title 'Android Emulator - Pixel_10:5554' \
  --input-policy foreground-preferred \
  --no-overlay \
  --store-root /tmp/auv-window-click-text-qemu \
  --json \
  --detail
```

The result was:

```text
run_id: 019fb956-d421-7b32-a2cf-b74403c7d00e
status: completed
matched_text: Native button
point: 312,89
selected_path: window_targeted_mouse
attempts: [{ path: window_targeted_mouse, succeeded: true }]
focus_disturbance: none
```

This reproduces the exact `window.clickText` question: OCR resolved the text,
but `ForegroundPreferred` stopped after the background event-post operation
reported no API error.

Two control paths changed Android state:

- `adb -s emulator-5554 shell input tap X Y`
- after raising the emulator window with AX, `screen.clickText`, which uses the
  global HID path

That isolates the failure to host-window targeted delivery into the emulator,
not OCR or the window-local coordinate conversion.

## AUV behavior before the fix

`ForegroundPreferred` is exposed by the CLI. It has not been removed. The
problem is the click attempt ordering and success definition:

```text
WindowTargetedMouse -> if API error, then foreground HID
                    -> if API accepted event, stop
```

The driver can observe that it constructed and posted an event; it cannot
observe whether QEMU accepted it or whether Android handled it. The current
`succeeded: true` field therefore means transport dispatch succeeded, not
semantic click success.

There is also an internal policy inconsistency: the scroll implementation's
`ForegroundPreferred` ordering starts with foreground HID, while click starts
with window-targeted background delivery.

## CUA

Current CUA source has two explicit click delivery modes:

- `background`: AX action or process-targeted CGEvent without fronting
- `foreground`: briefly front the concrete window, perform the action, allow
  transient UI to settle, then restore the previous frontmost application

CUA labels both forms `verified: false`; its tool description tells the agent
to use a ladder of background AX, screenshot, background pixel, screenshot,
then foreground delivery. Its global desktop click posts at the HID event tap,
not to a PID.

CUA's foreground helper is window-centric rather than bundle-ID-centric. It
uses the target PID and window ID with WindowServer/SkyLight process and window
operations, so a bundleless executable is not inherently excluded. These are
private macOS interfaces and remain best-effort.

For Android, CUA also has an ADB transport. It captures via
`adb exec-out screencap -p`, executes guest shell commands through ADB, and has
a root/multitouch route using `sendevent`. This avoids relying on whether the
host QEMU window forwards a background CGEvent into the Android guest.

No QEMU-specific exception was found in CUA's macOS process-targeted mouse
implementation. The important handling is architectural: explicit foreground
escalation for host windows and a guest-aware transport for Android.

The CUA binary was not installed locally, so upstream CUA was not executed
against this emulator. AUV's window-targeted path uses the same family of
process-targeted CGEvent techniques, and that path was tested directly.

## KWWK

KWWK's click flow prefers an accessibility action when an element supports one
and otherwise uses `BackgroundMouseDispatcher`. The dispatcher stamps PID,
window-number, and window-local fields on CGEvents and finishes with
`event.postToPid(targetPID)`.

KWWK adds a `BackgroundActivationSession`: it posts AppKit-defined activation
events and a window-center mouse primer while suppressing focus messages that
would disturb the previously foreground application. This is more elaborate
than a bare `postToPid`, but it still cannot prove that a guest consumed the
event. No Android Emulator or QEMU special case was found.

KWWK does compare snapshots for some operations. For example, its scroll output
warns when neither an AX scroll nor a `postToPid` fallback produces observable
state change. Its click API, however, does not expose a CUA-style explicit
foreground delivery rung.

The KWWK binary was not installed locally, so this conclusion is from current
source inspection. Given its final mouse transport, the expectation that it
can silently no-op on this QEMU window is an inference, not a direct KWWK run.

## QEMU focus and mouse-grab boundary

Upstream QEMU's macOS Cocoa frontend documents and implements the relevant
boundary explicitly. Its stable source says button events must not be sent to
the guest unless the mouse is grabbed or the window has focus; a click on a
background window is treated as activation and intentionally not passed
through. Current source continues to track `isMouseGrabbed`, and mouse-up can
establish the grab after an earlier mouse-down was not accepted as guest input.

Android Emulator uses Google's QEMU-derived emulator and a Qt host UI, so the
upstream Cocoa source is not proof of its exact event handler. It is strong
supporting evidence for the general host-window/guest-input boundary, while the
local AUV, global-HID, and ADB controls are the direct evidence for this case.

## Other computer-use frameworks

Anthropic's reference computer-use environment runs a controlled Linux desktop
inside Docker and sends input with `xdotool` inside that desktop. Like ADB, this
places input in the environment that owns the application instead of trying to
deliver a background mouse event through an unrelated macOS host window.

This is the common reliable pattern for VM, container, and mobile targets:
inject through the guest/control plane when one exists; use host global input
only when deliberately operating the visible foreground desktop.

## Implemented AUV slice

The owner approved and the follow-up implemented this narrow bug fix:

1. Click `ForegroundPreferred` now attempts foreground first, matching
   its name and the scroll policy.
2. Foreground activation addresses the owning process by PID before falling
   back to bundle ID or application name, so bundleless QEMU processes can
   participate.
3. `InputActionResult` now persists `verified`. Raw input producers return
   `verified: false`; only a producer with post-action read-back may set it to
   `true`.
4. The driver does not automatically retry foreground after every
   API-successful background
   click inside the low-level driver: without semantic evidence it would cause
   duplicate clicks on applications where the first click worked. Let a
   verification-aware operation or caller select the next rung.
5. A typed Android/ADB capability remains a separate, owner-approved feature;
   do not hard-code a QEMU exception into the generic macOS mouse driver.

The post-fix live run used the same text and window:

```text
run_id: 019fb970-d397-74f1-971d-ca8f93422129
selected_path: foreground_system_events
attempts: [{ path: foreground_system_events, succeeded: true }]
verified: false
focus_disturbance: foreground
Android UIAutomator: tap 4 -> tap 5
```

This is direct evidence that foreground delivery reached this Android guest.
It remains intentionally unverified in the driver result because UIAutomator
was an external test assertion, not an observation performed by the click
operation itself. A planned repeat could not run because the emulator process
and ADB device exited after the first confirmed run; that later environment
loss does not change the captured `tap 4 -> tap 5` result.

## Sources

- [CUA macOS click tool](https://github.com/trycua/cua/blob/main/libs/cua-driver/rust/crates/platform-macos/src/tools/click.rs)
- [CUA macOS mouse transport](https://github.com/trycua/cua/blob/main/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs)
- [CUA macOS foreground helper](https://github.com/trycua/cua/blob/main/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs)
- [CUA Android ADB transport](https://github.com/trycua/cua/blob/main/libs/python/cua-sandbox/cua_sandbox/transport/adb.py)
- [KWWK computer-use actions](https://github.com/EYHN/kwwk-computer-use-core/blob/main/Sources/KWWKComputerUseCore/ComputerUseActions.swift)
- [KWWK background input dispatcher](https://github.com/EYHN/kwwk-computer-use-core/blob/main/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift)
- [KWWK background activation session](https://github.com/EYHN/kwwk-computer-use-core/blob/main/Sources/KWWKComputerUseCore/BackgroundActivationSession.swift)
- [QEMU stable Cocoa frontend](https://gitlab.com/qemu-project/qemu/-/blob/stable-6.1/ui/cocoa.m)
- [QEMU current Cocoa frontend](https://gitlab.com/qemu-project/qemu/-/blob/master/ui/cocoa.m)
- [Anthropic computer-use reference tool](https://github.com/anthropics/claude-quickstarts/blob/main/computer-use-demo/computer_use_demo/tools/computer.py)
