# Find text in a window

Captures a window and returns every match of a text query, with its screen
bounds and recognizer confidence. The response also carries the window and
the capture the matches came from.

## Matching

The query matches recognized text case-insensitively and as a substring.
Matches keep the recognizer's order, and the first one is what clients treat
as the best match. An empty result is not an error.

## Coordinates

Match bounds are logical screen rectangles, the same space as window frames
and click positions, so a match can be clicked directly.

## The capture

The capture is a reference held by the Runner, not pixels: fetch them with
`captures.image` when you need them. It carries a ThumbHash for a quick
preview. A reference expires when the Runner evicts it, and then fails with
`NOT_FOUND`.

## Regions

`screen_region` limits the search to a logical screen rectangle, clipped to
the window; `region` does the same in fractions of the window. Set one, not
both.

## Examples

### Click the first match

```ts
const { matches } = await window.findText('Play')
if (matches[0])
  await window.click(matches[0])
```

```rust
let found = window.find_text("Play").await?;
if let Some(best) = found.matches.best_match() {
  window.click(auv_driver::ScreenPoint::from(best.action_point()), Default::default()).await?;
}
```

### From the command line

```sh
auv invoke window.findText Play --target com.netease.163music
```
