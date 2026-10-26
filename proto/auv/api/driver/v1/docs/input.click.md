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

`capture_precondition` binds the click to a full `CaptureWindow` or
`CaptureDisplay` reference from the same Runner. Pixels stay on the Runner.
Without this field, the call retains unguarded delivery.

The Runner captures the same resource at the original resolution before
calling input delivery. It requires exact equality of pixels and capture
metadata. It also compares the visible window list before and after capture.
Window movement, resize, display geometry changes, scrolling, navigation,
and visible dialogs invalidate the click when they change this evidence.
Changes to other visible windows also invalidate the click.
Animation, caret blinking, and backend changes can therefore cause refusal.

A window capture requires the same target window. A display capture requires
the click point to remain inside that captured display.
Expired, evicted, foreign, or unknown references refuse input.
An unreadable target or an unstable capture also refuses input.
Region, OCR-produced, and streamed captures currently have no click baseline
and return `CHECK_UNAVAILABLE`.

Refusal returns `FAILED_PRECONDITION` with `ClickPreconditionFailure` encoded
directly in `grpc-status-details-bin`. Every reason requests a new capture
and a new target decision. CLI and MCP return `recapture_required`.
The Runner records `auv.input.click_refused` in traced runs, including the
capture ID, reason, `new_capture` recovery, and `input_delivered: false`.
Refusal calls no input capability, including activation or modifier delivery.

The check and delivery are not atomic. The UI can change after the final
capture, during native activation, fallback, or event submission.
Repeated clicks also retain their interval between events.
Changes absent from both pixels and window metadata cannot be detected.
Window lists do not provide a portable guarantee about every occluder.
This policy does not establish semantic success or trigger automatic replanning.
Guarded callers require a Runner that implements this field.

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
Local execution without a selected Runner refuses this argument.
Runner-backed `window.capture` and `display.capture` return the reference in
`capture.capture_id`. This ID supplies the next click's `--capture` value.
