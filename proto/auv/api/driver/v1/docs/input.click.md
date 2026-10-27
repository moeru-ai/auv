# Click at a point

Clicks at a position. The position names its coordinate space: the screen,
a display, or a window.

## Window clicks

A window position, or any position sent with a target `window`, is delivered
to that window:

- a screen or display position is converted with the window's current frame;
- the point must lie inside the window, or the call fails with
  `INVALID_ARGUMENT`;
- `options.policy` chooses background or foreground delivery, and
  `options.window_strategy` the background route.

The response reports the refreshed window and the delivered point in both
window and screen space.

## Global clicks

A screen or display position without a target window is a global click at
that point, whatever is there. It has no target window, so `policy` and
`window_strategy` are rejected instead of ignored.

## Delivery is not success

The result describes how the click was delivered (the path, attempts and
disturbance to the user). It does not prove the app reacted; check the app's
state separately.

## Capture precondition

`ClickPointWithCapture` requires `click` (a `ClickPointRequest`) and `capture`
(a full `CaptureWindow` or `CaptureDisplay` reference from the same Runner).
Its response contains `result` (a `ClickPointResponse`). Pixels stay on the Runner.
Ordinary `ClickPoint` retains unguarded delivery.

SDK capture options select the dedicated method. An older Runner returns
`UNIMPLEMENTED` without input. Clients never retry through `ClickPoint`.

The Runner captures the same resource at the original resolution before
calling input delivery. It requires exact equality of pixels and capture
metadata. Window captures compare visible windows that intersect the target.
Display captures compare visible windows that contain the click point.
Both comparisons retain the backend order and use snapshots before and after capture.
Window movement, resize, display geometry changes, scrolling, navigation,
and visible dialogs invalidate the click when they change this evidence.
Unrelated window metadata outside that area does not invalidate the click.
The pixel comparison still covers the full captured image.
Animation, caret blinking, and backend changes can therefore cause refusal.

A window capture requires the same target window. A display capture requires
the click point to remain inside that captured display.
Expired, evicted, foreign, or unknown references refuse input.
An unreadable target or an unstable capture also refuses input.
Region, OCR-produced, and streamed captures currently have no click baseline
and return `CHECK_UNAVAILABLE`.

Refusal returns `FAILED_PRECONDITION` with `ClickPreconditionFailure` packed
in `google.rpc.Status.details` as `Any`. Every reason requests a new capture
and a new target decision. CLI and MCP return `recapture_required`.
The Runner records `auv.input.click_refused` in traced runs, including the
capture ID, reason, a bounded diagnostic `cause`, `new_capture` recovery, and `input_delivered: false`.
The status message also includes the cause. Unknown or malformed typed details
preserve the original RPC error in both SDKs.
Refusal calls no input capability, including activation or modifier delivery.

The check and delivery are not atomic. The UI can change after the final
capture, during native activation, fallback, or event submission.
Repeated clicks also retain their interval between events.
Changes absent from both pixels and window metadata cannot be detected.
Window lists do not provide a portable guarantee about every occluder.
This policy does not establish semantic success or trigger automatic replanning.
Linux window-targeted guards and AT-SPI window crops return `CHECK_UNAVAILABLE`.
AT-SPI extents cannot establish reliable screen placement on GNOME Wayland.
Linux callers can use display captures with screen or display coordinates.

Recapture runs inside the input task and delays other input operations.
Exact pixels can reject an unchanged target because of caret blinking or animation.
See the [review evidence and measured limits](../../../../../../docs/ai/references/driver/2026-10-26-click-capture-precondition-contract.md#review-evidence-and-limits).

## Examples

### Click inside a window

```ts
await window.click({ x: 120, y: 48 }) // window-local point
await window.click(match)             // the center of a text match
```

### Global click

```ts
await device.input.click({ x: 640, y: 400 })
await device.input.click({ coordinateSpace: { case: 'displayId', value: display.displayId }, x: 10, y: 10 })
```

```rust
runner.input().click(&auv_driver::ScreenPoint::new(640.0, 400.0), Default::default()).await?;
```

### From the command line

```sh
auv invoke input.clickPoint 640 400
auv invoke input.clickPoint 120 48 --target com.netease.163music
```

### Click from a capture

```ts
const { capture } = await window.capture()
await window.click({ x: 120, y: 48 }, undefined, { capturePrecondition: capture! })
// ClickPreconditionError carries captureId, reason, and recovery: 'new_capture'.
```

```rust
window.click_with_capture(
  auv_driver::WindowPoint::new(120.0, 48.0),
  Default::default(),
  Some(&capture.reference),
).await?;
```

```sh
auv --run <run-id> invoke input.clickPoint 120 48 --target window:<window-id> --capture <capture-id>
```

The CLI and MCP `capture` argument requires the Run that owns the capture.
Local execution without a selected Runner returns `invalid_target` and requests
selection through `--run`. Recapture alone cannot fix this configuration.
Frontend target-resolution failures return `recapture_required` in the command
record, but currently have no `auv.input.click_refused` Runner event.
Runner-backed `window.capture` and `display.capture` return the reference in
`capture.capture_id`. This ID supplies the next click's `--capture` value.
