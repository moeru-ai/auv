# SDK ergonomics, and `auv` as the SDK client

Date: 2026-10-09

Status: **Phases 1 (#295), 2 (#296) and 3 implemented.** Names marked *provisional* are open.

## Problem

Playground scripts have two APIs. `auv.*` is a curated script API, wired by
hand through four layers: script types, the worker proxy, host bindings
(`runtime/bindings.ts`) and a per-method `Backend`. `device.*` is the plain
`@auv-js/sdk` Runner client. Since #283 and #293, the timeline, previews,
replay and the mock desktop all work at the transport. The hand-wired
`auv.*` layers therefore add only three things the SDKs lack:

1. **Areas.** `area(window).below(40)`, `.region({ top: '10%', height: 80 })`
   and `.inset(8)` build screen rectangles from what a script holds.
2. **Short inputs.** `{ bundleId }` instead of a selector `oneof`,
   `'cmd+a'` instead of `['cmd', 'a']`, and `click(area)`.
3. **Handle lineage.** Results carry `$ref`, so the inspector shows which
   call produced a value and which calls used it, and can pin it.

The Rust client (`auv-core`) has the same gaps. It has no areas. Its
`WindowClient` has no keyboard methods: JS gained `typeText` and `pressKeys`
on windows in #282. Its `press_keys` takes structured options only, although
`Key` and `Modifier` already parse from strings.

## Decisions (owner, 2026-10-09)

- Area geometry goes into **both SDKs** as plain rectangle operations: `Rect`
  methods in Rust `auv-driver-common`, and pure functions in JS
  `@auv-js/sdk`. Neither SDK gets an `Area` object. The chaining `area()`
  wrapper and `named()` (a canvas label) are playground script features, so
  they stay in the playground, built on the SDK functions.
- After the switch, playground scripts keep **one global, `auv`**: the SDK
  Runner client for the selected Device and the current Run. `device` is
  removed, with no alias; AUV is still 0.0.x.
- Improve `auv-core` together with the JS SDK, so that the two expose the
  same shapes.

## Phase 1: shared ergonomic API (Rust and JS)

| Need | JS `@auv-js/sdk` | Rust `auv-core` / `auv-driver-common` |
|---|---|---|
| Strips beside a rectangle | `above(rect, h, gap?)`, `below`, `leftOf`, `rightOf` | `rect.above(h, gap)`, `below`, `left_of`, `right_of` |
| Shrink or move | `inset(rect, n \| { top, right, bottom, left })`, `offset(rect, dx, dy)` | `rect.inset(n or Insets)`, `rect.offset(dx, dy)` |
| Box by edges, with percentages | `region(rect, { top: '10%', height: 80 })` | `rect.region(Edges { top: Some(Length::Percent(10.0)), height: Some(80.0.into()), .. })` |
| Point at a fraction | `at(rect, 0.5, 0.5)`, `center(rect)` | `rect.at(0.5, 0.5)`, `rect.center()` (exists) |
| Contains, overlap | `contains`, `intersect` (#292) | `rect.contains_point(p)`, `rect.contains_rect(r)`, `rect.intersect(other)` |
| Select a window by app | `windows.resolve({ bundleId })` (`WindowQuery`) *and* the full selector | `WindowSelector::main_visible().owned_by(App::bundle(..))` (exists) |
| Keys as a string | `window.pressKeys('cmd+a')` or `['cmd', 'a']`; `splitKeyCombination` | `window.press_keys("cmd+a")` (new on `WindowClient`); `split_key_combination` |
| Type into a window | `window.typeText(text)` (exists) | `window.type_text(text)` (new on `WindowClient`) |
| Click a text match or rect | `window.click(match)` (exists, `bounds`) | `window.click(&match)`: `OcrMatch` and `Rect` convert to a screen `PointerTarget` at their center |

Rules both sides keep:

- An area is a plain screen rectangle. Its helpers never convert
  coordinate spaces, and percentages are relative to the area they are
  applied to.
- `region()` takes two of start, end and size per axis. A missing start is
  0, a missing size fills the rest, and all three at once is an error. These
  are the playground's current rules, moved as is.
- Key strings use `+` between keys, with a trailing `+` meaning the plus key
  (`cmd++`). These are the playground's current rules.

- `WindowQuery` takes at most one application field (`bundleId`, `appName`,
  `pid`, `frontmost`) and one title field (`title`, `titleContains`). More is
  a `TypeError` before any call. No application means the frontmost app; no
  title means its main visible window.

The playground's `area(source).below(40).named('Search')` keeps its chaining
and label, and its geometry now calls these functions.

Names *provisional*: `Insets`, `Edges`, `Length::{Points, Percent}`,
`WindowQuery`.

## Phase 2: playground lineage by IDs

The inspector's lineage (produced by, bound to, used by) keyed on `$ref`
strings that only `auv.*` results carry. SDK results carry stable IDs
instead: `CaptureRef.captureId`, `WindowRef.windowId` and `displayId`.

- **Keys.** Playground resources are keyed by those IDs: `window:<windowId>`,
  `display:<displayId>` and now `frame:<captureId>` (it was a per-page
  counter). `runtime/rpc-resources.ts` registers SDK responses under them. A
  capture seen again in the same run keeps its first producer.
- **Users.** A call uses a resource when its request names the ID anywhere:
  `window.windowId`, `source.captureRef.captureId`, a `Position`'s window, or
  a script handle's `$ref` (`handles.ts` `mentionedRefs`).
- **Values.** `refOf` recognizes what a bound value stands for: a `Window`,
  `CapturedFrame`, `Display`, `WindowRef` or `CaptureRef` message, a
  `WindowClient`, or a script handle. Hover, pins, `focus()`, `area()`'s
  source and the value chips use it. Values that only mention a resource,
  such as a capture response, render as data with chips inside.
- Text results and input receipts have no Runner ID, so SDK values holding
  them link to their producing call only
  (`NOTICE(lineage-without-ids)` in `handles.ts`).

## Phase 3: switch `auv` to the SDK client

- The `auv` global is the SDK Runner client for the selected device and the
  current Run; `device` is removed with no alias. `sdk` stays as the module.
- The worker proxy, `runtime/bindings.ts`, the `auv.*` script API types and
  the per-method `Backend` methods are deleted. A `RunBackend` has Run
  lifecycle, `sdk()` and canvas pixels (`captureImage`); a selected `Backend`
  adds the canvas's own display listing and live captures, the AX tree and
  `dispose`. A recording now holds only SDK calls and canvas images.
- Resource handle types (`FrameHandle`, `WindowHandle`, …) moved from the
  script API to `handles.ts`: they are host records, not script values.
  `script-api/api.ts` keeps only the playground helpers (`area()`, `draw()`,
  `focus()`, `show()`, `centerOf()`, `sleep()`) and their types.
- An area has a `bounds` getter, so SDK clicks and scrolls take it at its
  center in screen space.
- The README, the examples and the editor's default script use the SDK API.
  The editor types `auv` as `any` until `TODO(playground-sdk-types)`.
- Known regression: `scrollUntil` has no client defaults, so scripts pass
  `maxSteps`, `noMotionConfirmations` and `settle` themselves
  (`TODO(sdk-scroll-until-defaults)`).

Each phase is one PR, in this order.

## Open questions

None open. `area()`'s `from` field now holds the ID-based key from `refOf`.
