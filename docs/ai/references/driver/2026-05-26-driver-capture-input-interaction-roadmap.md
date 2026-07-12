# Driver Capture, Low-Disturbance Input, and Interaction Roadmap

Date: 2026-05-26

Status: Draft roadmap. Names in this document are provisional unless they
already exist in code or `docs/TERMS_AND_CONCEPTS.md`.

## Purpose

This document records the current roadmap for improving AUV's lower-level
automation foundation before modeling Surface reconstruction.

The immediate goal is not to build a new recipe DSL and not to finalize a
Surface IR. The immediate goal is to make AUV faster, less disruptive to the
user, and easier to use from programmable Rust APIs. Once capture, input, focus,
and interaction primitives are clean, Surface reconstruction should become a
natural layer above them instead of a workaround for missing driver capability.

## Current Direction

AUV should first strengthen three areas:

1. Capture fast path
   - Reduce screenshot latency.
   - Avoid unnecessary file-system and PNG encode/decode round trips.
   - Add timing telemetry so slow steps are visible.
   - Keep artifact persistence asynchronous or recording-layer-owned.

2. Low-disturbance platform capabilities
   - Add no-steal coordinate click and keyboard input where the platform
     supports it.
   - Add AX semantic input paths on macOS.
   - Add background activation, synthetic focus, and focus-steal prevention as
     explicit platform capabilities.
   - Record selected delivery paths and fallback reasons.

3. Interaction core
   - Replace sub-recipe style composition with programmable Rust interactions.
   - Treat scroll scan, wait-until, pagination, and list traversal as reusable
     meta interactions.
   - Compose capture, recognition, input, timing, stability checks, and trace
     recording without inventing a new manifest language first.

Surface reconstruction remains important, but it is intentionally deferred.

## Non-Goals For This Roadmap

- Do not design the final Surface IR here.
- Do not prioritize YOLO, object detection, or model inference integration yet.
- Do not make NetEase Cloud Music a hardcoded architecture concept.
- Do not copy CUA, KWWK, or MaaFramework directly.
- Do not treat macOS AX as the cross-platform object model.

CUA, KWWK, and MaaFramework are reference points for reusable capabilities:
low-disturbance input, focus handling, capture speed, method selection,
recognition/action composition, ROI/target separation, and interaction loops.

## Existing Capability Comparison

This table preserves the current comparison between CUA, KWWK, and AUV. It is
included here so follow-up implementation specs can inherit the same baseline.

| Ability | CUA | KWWK | AUV | Explanation |
|---|---|---|---|---|
| App / window discovery | Yes | Yes | Yes | All three can enumerate app/window surfaces. AUV has `debug.listWindows` and typed selector work. |
| Window capture | Yes | Yes | Yes | AUV has capture contracts and artifact recording, but the fast path needs work. |
| Display / region capture | Yes | Yes | Yes | AUV has display, region, and window capture commands. |
| AX tree observe | Yes | Yes | Yes | All three can observe AX. CUA/KWWK more directly bind AX nodes to later actions. |
| Snapshot-bound element cache | Yes | Yes | Partial | CUA/KWWK require `element_index` from a prior snapshot/cache. AUV does not yet have a unified action target cache. |
| OCR text detection | Yes | Limited / auxiliary | Yes | AUV has relatively rich OCR commands: screen/window text, rows, and image text. |
| Row / list detection | Partial | Weak | Yes | AUV has visible row detection and scan commands, but the interaction abstraction is still immature. |
| Image / icon matching | Yes | Not prominent | Yes | AUV has `debug.findIconMatch`, but it is not yet integrated into a common interaction API. |
| AX semantic click | Yes | Yes | Yes | CUA/KWWK use `AXUIElementPerformAction`; AUV has AX press commands. |
| AX set focus | Yes | Yes | Yes | AUV has AX focus text input, but it is not yet part of a general text-entry strategy. |
| AX set value / selected text | Yes | Partial | Missing / incomplete | CUA has `AXSelectedText -> verify -> fallback`. AUV mostly uses System Events and clipboard paste today. |
| Coordinate click | Yes | Yes | Yes | All have coordinate click. CUA/KWWK favor target pid/window delivery; AUV currently has more foreground pointer paths. |
| Coordinate click without moving the real cursor | Yes | Yes | Missing / partial | CUA/KWWK use `SLEventPostToPid` / `CGEvent.postToPid` and window-local routing. AUV lacks a mature no-steal coordinate click path. |
| Real HID fallback | Yes | Possible fallback | Yes | This is high compatibility but disruptive. AUV has pointer commands that are closer to this layer. |
| Keyboard input | Yes | Yes | Yes | AUV has type, press key, and paste. CUA/KWWK emphasize background/pid-targeted delivery. |
| Paste with clipboard restore | Yes | Not prominent | Yes | AUV already has useful clipboard-preserving paste behavior. |
| Type fallback strategy | Yes | Medium | Missing / scattered | CUA is explicit: AX selected text first, then CGEvent fallback. AUV does not yet have a unified text strategy. |
| Submit key behavior | Yes | Yes | Yes | AUV supports submit keys, but this needs integration with text-entry strategy. |
| Scroll primitive | Yes | Yes | Yes | AUV has point/window-region scroll. KWWK tries AX scrollbar actions before wheel fallback. |
| AX scroll first | Partial | Yes | Missing / incomplete | AUV should learn from KWWK's AX-first scroll behavior. |
| Scroll verification | Partial | Yes | Partial | KWWK warns when fingerprint does not change. AUV scan records evidence, but ordinary scroll actions lack common verification. |
| Ghost / virtual cursor overlay | Yes | Yes | Yes | AUV has overlay work and `auv-overlay-macos`, but action integration is early. |
| Overlay does not eat mouse input | Yes | Yes | Yes | Click-through/no-activate overlay is the right direction for all three. |
| Overlay and input separation | Yes | Yes | Partial | CUA/KWWK clearly separate visual cursor and real input delivery. AUV still has historical command mixing. |
| Background activation | Yes | Yes | Missing / partial | CUA/KWWK both have background/focus-without-raise-like mechanisms. AUV mostly foregrounds. |
| Synthetic focus | Yes | Yes | Partial | AUV can set AX focus in specific commands, but lacks a focus guard. |
| Focus steal prevention | Yes | Yes | Missing | CUA/KWWK prevent or restore focus if the target self-activates. |
| Action resolver | Yes, scattered | Yes, scattered | Missing / prototype | CUA/KWWK do resolution inside action functions. AUV has `smartPress`-style prototypes but no common resolver/executor. |
| Fallback reason recording | Partial | Partial | Partial | AUV has strong tracing, but not systematic selected delivery/fallback/focus fields. |
| Evidence artifacts | Yes | Some | Strong | AUV's `.auv/runs`, artifacts, and inspect workflow are strengths. |
| Run recording / inspect | Partial | Weak | Strong | This remains an AUV differentiator. |
| Replay trajectory viewer | Yes | Overlay base | Partial | CUA has trajectory viewing; AUV can extend inspect/replay. |
| High-level recipe / workflow | Partial | Weak | Yes | AUV has recipes and music commands, but the direction is Rust programmable interactions rather than more recipe nesting. |
| Typed Rust driver API | Yes | Swift-first | In progress | AUV has `auv-driver` / `auv-driver-macos`, but the API is not complete. |
| Cross-platform overlay | Yes | macOS-first | macOS-first | CUA has platform overlay implementations; AUV is macOS-first now. |
| Private macOS SPI usage | Yes | Yes | Little / not systematic | CUA/KWWK use SkyLight/PostToPid-related paths. AUV should isolate these behind macOS capabilities. |

## Low-Disturbance Capability Matrix

This table preserves the current no-steal input/focus capability analysis.

| Capability | Solves | Needed for no-steal input | Needed for no-steal focus | AUV status | Notes |
|---|---|---:|---:|---|---|
| AX action dispatch | Trigger controls without pointer click | Required | Important | Partial | `AXUIElementPerformAction`, useful for buttons and menus. |
| AX set focused | Target text fields without clicking | Important | Required | Partial | `AXFocused=true`; must be verified and restored when appropriate. |
| AX set value / selected text | Insert text without keyboard events | Required | Important | Missing / incomplete | CUA's `AXSelectedText -> verify -> fallback` is the reference. |
| AX main/focused window synthetic focus | Make a target window behave internally as active | Important | Required | Missing | Write `AXMain` / `AXFocused` on window where safe. |
| AX capability detection | Know which actions/attributes are supported | Required | Required | Partial | Needed for `AXPress`, `AXSetValue`, `AXFocused`, etc. |
| Snapshot-bound AX element target | Act on observed nodes instead of guessing | Required | Important | Missing / scattered | Needed before generalized node actions. |
| AX action verification | Confirm semantic action changed state | Important | Important | Partial | Examples include after-observe, fingerprint, AXValue readback. |
| pid-targeted CGEvent mouse | Coordinate click without moving real cursor | Required | Important | Missing | macOS `CGEvent.postToPid` / `SLEventPostToPid`. |
| window-local CGEvent routing | Hit a concrete window instead of global pointer path | Required | Important | Missing | Requires window id, local point, and private routing fields/SkyLight on macOS. |
| pid-targeted keyboard event | Type without relying on the frontmost app | Important | Required | Missing | Current AUV is closer to System Events/foreground behavior. |
| background activation event | Make background target accept input | Important | Required | Missing | KWWK posts activation-like events. |
| focus-without-raise | Focus the target without raising its window | Important | Required | Missing | CUA has SkyLight/private SPI references. |
| focus steal suppression | Restore current app if target self-activates | Important | Required | Missing | Workspace observer / event tap style behavior. |
| foreground restore | Reduce harm after unavoidable foreground fallback | Auxiliary | Required | Missing | Not true no-steal, but useful. |
| overlay click-through cursor | Visual cursor that does not eat input | Auxiliary | Auxiliary | Partial | Important for debug/inspect, not the actual input path. |
| action executor | Select delivery and fallback for single actions | Required | Required | Missing / prototype | Should replace scattered command-specific resolution. |
| fallback policy | Decide when to upgrade to more disruptive paths | Required | Required | Missing | Example: forbid HID/foreground fallback by default. |
| disturbance metadata | Record potential mouse/focus disturbance | Required | Required | Partial | AUV has disturbance terms but should bind them to concrete delivery. |
| execution trace | Record selected delivery, fallback reason, focus behavior | Important | Important | Partial | AUV tracing is strong but lacks these fields. |
| capability probing | Detect supported backends at runtime | Important | Important | Partial | Include permissions and native backend availability. |
| permission model | Explain missing Accessibility/Screen Recording/Automation grants | Required | Required | Partial | AUV has permission probes; extend them for input backends. |
| safe default mode | Forbid real HID / foreground activation by default | Required | Required | Missing | Prevent accidental user disruption. |
| explicit fallback opt-in | Allow high-disturbance fallback only when requested | Required | Required | Partial | Existing `max_disturbance` is close but too coarse for typed APIs. |

## Missing And Incomplete Capability List

This section intentionally excludes abilities that are already adequate for the
next stage. It lists only missing or incomplete areas.

### Platform / Driver Capabilities

| Capability | AUV status | Required improvement | Priority |
|---|---|---|---|
| Capture backend selection | Incomplete | Define capture methods and select by capability, speed, and disturbance. | P0 |
| Fast window capture | Incomplete | Add a faster macOS path, likely ScreenCaptureKit first with fallback. | P0 |
| In-memory capture | Incomplete | Return image buffers/data to callers without mandatory temp PNG writes. | P0 |
| Capture resize / format policy | Incomplete | Standardize max dimension, PNG/JPEG/raw behavior per consumer. | P0 |
| Region crop from existing capture | Incomplete | Crop in memory when possible instead of re-capturing. | P0 |
| Capture timing telemetry | Missing | Record enumerate/capture/encode/write/OCR timings. | P0 |
| Window freshness validation | Incomplete | Detect stale window handles and refresh predictably. | P1 |
| Capability probing | Incomplete | Query supported capture/input/activation methods per target. | P1 |
| No-steal coordinate click | Missing | Add pid/window-targeted coordinate input on macOS. | P0 |
| No-steal keyboard input | Missing | Add target-window/pid keyboard delivery where possible. | P0 |
| AX set value / selected text | Missing | Add text insertion via AX, verify, then fallback. | P0 |
| AX synthetic focus | Incomplete | Integrate AX focus into typed text/click action paths. | P1 |
| Background activation | Missing | Add background activation or focus-without-raise capability. | P1 |
| Focus steal prevention | Missing | Detect target self-activation and restore prior frontmost app. | P1 |
| Input delivery strategy | Missing | Add explicit low-disturbance constraints and fallback rules. | P0 |
| Action execution report | Incomplete | Return selected delivery, attempts, fallback reason, timing, disturbance. | P0 |
| Clipboard as strategy | Incomplete | Integrate clipboard paste into text-input fallback strategy. | P1 |
| Drag / hold / touch / relative move | Missing | Needed for richer game/application automation. | P2 |
| ADB/CDP method model | Missing | Needed later for Android/browser backends. | P2 |

### Recognition / Extraction

YOLO and model inference are not part of the first implementation wave. The
near-term recognition work is only what capture and interaction need.

| Capability | AUV status | Required improvement | Priority |
|---|---|---|---|
| OCR over in-memory image | Incomplete | Avoid temp PNG writes for OCR when possible. | P0 |
| OCR over cropped region | Partial | Run OCR on an in-memory ROI/crop with correct coordinate projection. | P0 |
| Template/NCC matching over capture | Incomplete | Normalize ROI, threshold, multi-hit result shape. | P1 |
| Recognition timing | Missing | Record OCR/NCC/CV timings separately. | P0 |
| Recognition evidence schema | Incomplete | Normalize source image, ROI, box, confidence, rejected candidates. | P1 |
| Freeze / stability detection | Missing | Detect stable screen/list state before and after interactions. | P1 |
| Change / fingerprint detection | Incomplete | Support scroll/list no-change and end detection. | P1 |

### Interaction / Meta Primitive

| Capability | AUV status | Required improvement | Priority |
|---|---|---|---|
| ActionExecutor | Missing | Single-action executor for click/type/scroll/key with delivery choice/report. | P0 |
| Fallback policy | Missing | `background_only`, `prefer_background`, `foreground_allowed` style constraints. | P0 |
| Scroll scan interaction | Existing but needs replacement | Compose capture, recognition, scroll, stability, and end detection in Rust. | P1 |
| ROI / box / target separation | Incomplete | Adopt the useful Maa distinction between search area, detected box, and action target. | P1 |
| Named anchors / previous hits | Missing | Reuse prior recognition hits without sub-recipes. | P1 |
| Wait-until recognition | Partial | Generic wait loop over recognizers and predicates. | P1 |
| End-of-list detection | Incomplete | Combine no-new-items, fingerprint-stable, scrollbar-end, and max-step guards. | P1 |
| Interaction trace | Incomplete | Record each iteration's capture, recognition, action, and stop reason. | P1 |
| Custom recognition hook | Missing | Later Rust/JS extension point. | P2 |
| Custom action hook | Missing | Later Rust/JS extension point. | P2 |

## Capture Fast Path Project

Capture performance should be the first derived spec.

### Problem

AUV has working capture, but current paths frequently pay avoidable costs:

- Re-enumerating windows before capture.
- Capturing through `xcap` even when a faster platform path may be available.
- Writing temporary PNG files for OCR or evidence before the action path really
  needs persistence.
- Encoding PNG in the operation path when the consumer only needs an image
  buffer or a cropped region.
- Lacking timing telemetry, so it is hard to see whether slowness comes from
  enumeration, capture, encoding, file I/O, OCR, or recording.

CUA's Swift implementation is a useful reference because it uses
ScreenCaptureKit for window/display capture, returns encoded image data, handles
max-dimension resizing, retries transient SCK streaming failures, and falls back
to `CGWindowListCreateImage` when needed. CUA's Rust implementation also
contains a simpler `screencapture` subprocess path, but that is not the path to
copy for performance.

### Direction

The first capture milestone should provide:

1. `CaptureFrame` or equivalent in-memory capture result.
2. Capture timing breakdown:
   - target resolution
   - window/display enumeration
   - capture backend time
   - resize/crop time
   - encode time
   - temp/artifact write time
   - OCR/recognition time when chained
3. macOS ScreenCaptureKit capture backend.
4. `CGWindowListCreateImage` fallback where useful.
5. Existing `xcap` path retained as fallback until the new path is proven.
6. Consumer-selected image output:
   - raw RGBA image
   - PNG bytes
   - JPEG bytes
   - cropped view
7. Recording-layer persistence:
   - action path returns image/data immediately
   - artifact filenames are allocated by the recorder
   - artifact writes may happen after the operation or through an async writer

### Boundary

The driver crate should not know about `.auv/runs` storage. It may return image
data, metadata, and optional source paths. The AUV runtime/recorder decides if,
where, and when artifacts are persisted.

## Low-Disturbance Input And Focus Project

This should be the second derived spec.

### Problem

AUV can click, type, paste, press keys, and use some AX operations, but many
paths still assume foreground/system input. That makes verification disruptive
and prevents AUV from becoming a reliable GUI-to-CLI automation runtime.

### Direction

Add a typed action execution foundation without requiring Surface IR:

- `InputMode` or a similarly reviewed term
  - `BackgroundOnly`
  - `PreferBackground`
  - `AllowForeground`
- `PrepareForInputOptions`
  - `KeepCurrent`
  - `Background`
  - `FocusWithoutRaise`
  - `Foreground { settle }`
- `InputPreparationLease`
  - previous frontmost application/window where available
  - focus guard or suppression state where installed
  - background activation state that needs restoration
- `InputActionResult`
  - selected delivery path
  - attempted paths
  - fallback reason
  - timing
  - mouse/focus disturbance
  - evidence references when recording exists

The public API should be grouped by automation target rather than backend
mechanism. Window-local actions belong under `session.window()`, screen-canvas
actions belong under `session.screen()`, display topology and display-local
capture belong under `session.display()`, and `session.input()` remains a raw
escape hatch for pointer, keyboard, clipboard, or paste primitives.

The public API should express allowed disturbance, not force callers to choose
private backend details. The result records what actually happened.

Example shape:

```rust
let lease = session.window().prepare_for_input(
  &window,
  PrepareForInputOptions {
    mode: InputMode::PreferBackground,
    preserve_frontmost: true,
    settle: Duration::from_millis(80),
  },
)?;

let report = session
  .window()
  .click(&window, point, Click::Single, ClickOptions::background_only())?;

session.window().restore_input(lease)?;
```

The first implementation should prioritize:

- AX press / AX pick.
- AX focus.
- AX selected text / AX value with verification.
- pid/window-targeted mouse events on macOS.
- pid/window-targeted keyboard events where feasible.
- clipboard paste as an explicit fallback strategy.
- background activation and focus-steal suppression as separate capabilities.

## Interaction Core Project

This should be the third derived spec.

### Problem

Recipes and command catalog functions currently absorb too much orchestration.
Scroll scan is the clearest example: it is not a primitive click/scroll, and it
should not require sub-recipes. It is a programmable interaction composed of
capture, recognition, action execution, stability checks, evidence, and loop
termination.

### Direction

Add Rust programmable interactions above driver primitives and recognition:

- `ActionExecutor` for one action at a target.
- `InteractionContext` for one programmable interaction run.
- reusable interactions:
  - `wait_until`
  - `scroll_scan`
  - `paginate`
  - `search_and_select`
- common concepts inspired by MaaFramework:
  - ROI/search area
  - detected box
  - action target
  - named anchor / previous hit
  - timeout/rate limit
  - wait for freeze/stability
  - on-error result reporting

These should be typed Rust APIs first. RPC/JS exposure can follow once the Rust
shape is proven.

## Why Not Surface First

Surface reconstruction depends on reliable lower layers:

- Fast capture is needed for iterative reconstruction.
- Low-disturbance input is needed for live verification.
- Interaction loops are needed for scrolling, pagination, and multi-step UI
  exploration.
- Recognition timing and evidence are needed to trust the reconstructed result.

If AUV models Surface before fixing these capabilities, Surface will be forced
to compensate for slow capture, foreground-only input, and brittle interaction
control. The resulting API would look like a workaround rather than a stable
automation foundation.

## Follow-Up Specs

This roadmap should spawn separate implementation specs:

1. `macos-capture-fast-path-design`
   - ScreenCaptureKit backend
   - in-memory capture
   - timing telemetry
   - async artifact persistence boundary

2. `low-disturbance-input-focus-design`
   - delivery modes
   - action reports
   - macOS PostToPid/window-local input
   - AX text strategy
   - focus safety

3. `interaction-core-design`
   - ActionExecutor
   - wait-until
   - scroll scan
   - ROI/box/target separation
   - anchors and loop trace

Surface reconstruction should be revisited after these three specs have enough
implementation behind them to support real examples and recipe replacement.

## Open Questions

- Should the first capture fast path live entirely in `auv-driver-macos`, or
  should the root crate keep a compatibility adapter during migration?
- Should `xcap` remain the default until ScreenCaptureKit is proven in local
  examples, or should ScreenCaptureKit become the default immediately with
  fallback?
- Should capture telemetry be returned directly in `CaptureFrame`, emitted as
  trace events by runtime adapters, or both?
- Should background input use private macOS SPI behind an explicit capability
  flag, or should the first pass use only public `CGEvent.postToPid` and AX
  operations?
- Should interaction anchors use the word `anchor`, following MaaFramework, or
  should AUV reserve that term for recognition hits only?

## Implementation Notes And Smells To Track

- Current capture code mixes target resolution, capture, encoding, and artifact
  persistence in several paths.
- Current typed `Window` is a data object, but its freshness/lifetime semantics
  are not explicit enough.
- Current input APIs such as global `click_at(point, click)` do not carry enough
  target identity for no-steal delivery.
- Current text input paths are split between System Events, clipboard paste, and
  command-specific behavior.
- Current scroll scan behavior is too recipe/command shaped; it should become a
  reusable Rust interaction.
- Current overlay code is useful, but should remain visual-only and not become
  the input mechanism.
