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
