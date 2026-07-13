# Background Scroll Policy Design

Date: 2026-06-02

## Status

Accepted for the first implementation slice. This spec records the intended
contract and module boundaries. It does not approve implementing every deferred
scroll backend in one slice.

## Problem

AUV currently exposes `InputPolicy::{BackgroundOnly, BackgroundPreferred,
ForegroundPreferred}` in `crates/auv-driver/src/input.rs`, but macOS scroll
does not honor the policy shape consistently.

The previous macOS `InputApi::scroll_at(screen_point, ...)` path in
`crates/auv-driver-macos/src/session.rs` rejected `BackgroundOnly`, then used
native `scroll_point` for other policies. The Swift implementation in
`Pointer.swift` warps the real cursor, posts a mouse move, posts a wheel event
to `.cghidEventTap`, then restores the cursor. That is a global/system input
path, not a background window-targeted path.

This matters for scan loops such as NetEase playlist listing. A scroll scan can
issue many scrolls. If `BackgroundPreferred` silently uses global HID, the scan
may disturb the user's active workflow while still looking like a background
operation in trace data.

## Policy Semantics

`InputPolicy` remains the top-level user intent:

- `BackgroundOnly`: only no-steal background delivery is allowed. If no
  background path succeeds, return an error.
- `BackgroundPreferred`: try background delivery first. If background paths
  fail, automatic foreground/global fallback is allowed, but the selected path,
  failed attempts, fallback reason, and disturbance metadata must be recorded.
- `ForegroundPreferred`: foreground/global delivery is acceptable as the primary
  reliable path for input actions. Simple non-input capabilities that are
  naturally background and non-disturbing, such as window capture/screenshot,
  should still remain background instead of activating a window just because the
  policy says foreground is acceptable.

The important distinction is attempt ordering, not whether fallback is possible:
`BackgroundPreferred` can fallback to foreground, but foreground must not be the
first and only attempt when a target-aware background API exists.
For scroll input, `ForegroundPreferred` should choose the foreground/global HID
path directly unless the operation is implemented by a non-disturbing capability
that is already known to be reliable.

## API Boundary

Scroll needs three layers with separate responsibilities.

### Low-Level Input

`InputApi` is the low-level escape hatch for raw/global input primitives. It
does not own target-aware policy orchestration because it only receives screen
coordinates or process-wide input data.

Planned shape:

```rust
session.input().scroll_global_hid(screen_point, scroll, settle)
```

The old `scroll_at(screen_point, ...)` name is removed. The explicit global
path is `scroll_global_hid(...)`; it must not be treated as a background scroll
API.

### Target-Aware Window Operations

`WindowApi` owns window-bound actions. It has the `Window`, window-local point,
screen point conversion, pid, window number, and enough context to choose
delivery attempts.

Planned shape:

```rust
session.window().scroll(window, window_point, scroll, options)
```

`WindowApi::scroll` applies the policy ladder and returns `InputActionResult`
with the actual selected path. Product crates should call this API instead of
converting window-local anchors to screen points and calling raw input.

### AX Capability

AX is a separate stateful capability, not just another `InputApi` primitive. AX
scroll depends on AX tree state, element identity/path, role, action names,
scrollbar attributes, settable values, and verification. It should either live
behind an explicit `AxApi` facade or remain a private macOS driver capability
called by `WindowApi::scroll`.

Current AUV already has AX tree capture, generic AX action, and AX focus
support, but it does not yet have a dedicated AX scrollbar scroll capability.
The first implementation slice should not implement AX scrollbar discovery,
AXIncrement/AXDecrement, or AXValue mutation. It should reserve the contract
surface and add explicit code-site `TODO(background-scroll-ax)` markers wherever
an AX scroll branch is intentionally skipped.

Implementation work that touches AX scroll must include code-site deferral
markers while the behavior is incomplete. Examples:

```rust
// TODO(background-scroll-ax): AX scrollbar action/value scrolling is deferred
// until the driver exposes scrollable AX node discovery and verification.
```

```swift
// TODO(background-scroll-ax): AXValue mutation for scroll bars is deferred
// until the Rust contract names the producer-side scroll evidence.
```

These markers are required anywhere an AX branch, enum variant, function, or
call site is intentionally reserved but not implemented.

## Delivery Paths

`InputDeliveryPath` should describe what actually happened. Planned additions:

- `AxScroll`
- `WindowTargetedWheel`
- `WindowTargetedKeyboardScroll`

`ForegroundSystemEvents` remains the global HID/foreground fallback path.

Do not reuse `WindowTargetedMouse` for wheel scrolls or keyboard-scroll delivery.
Scroll is difficult enough that traces must preserve the difference between
wheel, keyboard, AX, and global HID attempts.

## Attempt Ladder

The default `WindowApi::scroll` ladder should be:

1. AX scroll capability, if the target or window exposes a usable scrollable AX
   element. In the first implementation slice this remains a reserved candidate
   with `TODO(background-scroll-ax)` markers, not a working backend.
2. pid/window-targeted wheel event using window-local routing fields.
3. pid/window-targeted keyboard scroll, when explicitly enabled by strategy or
   caller hints.
4. global HID foreground/system scroll, only when policy allows foreground
   fallback. For `ForegroundPreferred`, this can be the first selected scroll
   input path.

Keyboard scroll is not assumed reliable. It is a candidate path, not a default
guarantee, because apps may ignore background keys, focus may not land on the
scrollable region, and arrow/page keys may move selection instead of scrolling.

Targeted wheel is also not assumed reliable. It should be attempted and
verified, not treated as proof of semantic scroll.

## Delivery Strategy

`ScrollOptions` should carry a scroll-specific delivery strategy so callers can
disable risky paths or change attempt order without reimplementing dispatch.
Use an ordered candidate list rather than independent booleans:

```rust
pub struct ScrollDeliveryStrategy {
  pub candidates: Vec<ScrollDeliveryCandidate>,
}

pub enum ScrollDeliveryCandidate {
  AxScroll,
  WindowTargetedWheel,
  WindowTargetedKeyboardScroll,
  ForegroundHid,
}
```

`ScrollDeliveryStrategy` is the pre-execution plan: which scroll delivery
candidates are allowed and in what order. `InputDeliveryPath` remains the
post-execution fact: which path was attempted or selected. The two names should
stay separate even when enum variants are similar.

Default background-first strategy:

```rust
ScrollDeliveryStrategy {
  candidates: vec![
    ScrollDeliveryCandidate::AxScroll,
    ScrollDeliveryCandidate::WindowTargetedWheel,
    ScrollDeliveryCandidate::ForegroundHid,
  ],
}
```

`WindowTargetedKeyboardScroll` should be omitted by default until app-specific
evidence supports it. `ForegroundHid` may appear in a strategy, but the policy
still controls whether it is legal: `BackgroundOnly` must skip or reject it,
while `BackgroundPreferred` may use it after background candidates fail.

Avoid broad one-off booleans at product call sites. Product crates can choose a
strategy preset or provide a short ordered candidate list, but they should not
own macOS fallback implementation details.

## Verification

Scroll success is not proven by event delivery. A scroll attempt is successful
only when post-action observation provides evidence such as:

- changed viewport fingerprint,
- changed recognized row set,
- changed scroll boundary evidence,
- or a typed AX state change from the scrollable element.

`InputActionResult` records input delivery. Scan code records observation
evidence. The scan loop should combine both when deciding whether to continue,
retry with another path, or report a diagnostic.

## NetEase Playlist Scan

`crates/auv-netease-music` should not call raw global input for sidebar
scrolling.

The scan should call a target-aware window scroll API:

```rust
session.window().scroll(
  &self.window,
  WindowPoint::new(anchor.x, anchor.y),
  Scroll::new(0.0, vertical_delta),
  options,
)
```

It should record selected path and attempts in scan interaction events or the
future durable trace spans. If `BackgroundPreferred` falls back to
`ForegroundSystemEvents`, that fallback must be visible in output artifacts.

## Module Placement

- `crates/auv-driver/src/input.rs`: cross-platform contract types such as
  `InputPolicy`, `ScrollOptions`, `InputDeliveryPath`, `InputActionResult`, and
  any strategy type.
- `crates/auv-driver-macos/src/session.rs`: `WindowApi::scroll` policy ladder
  and `InputApi` raw/global scroll entrypoint.
- `crates/auv-driver-macos/src/native/input.rs`: Rust wrappers for
  pid/window-targeted delivery primitives.
- `crates/auv-driver-macos/src/native/pointer.rs`: Rust wrapper for global and
  window-targeted wheel pointer functions.
- `crates/auv-driver-macos/src/native/ax_tree.rs`: AX capture/action/focus and
  future AX scroll wrappers.
- `crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Pointer.swift`:
  global HID scroll and pid/window-targeted wheel implementation.
- `crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Keyboard.swift`:
  pid/window key delivery for any future keyboard scroll path.
- `crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/AxTree.swift`:
  AX tree, AX action, AX focus, and future AX scrollbar action/value support.
- Product crates: provide target, policy, strategy hints, and verification
  rules. They do not own macOS input fallback policy.

## Implementation Slices

1. Contract and naming slice: add scroll delivery path names and replace
   `InputApi::scroll_at` with explicit global/system `scroll_global_hid`.
2. Window-targeted wheel slice: add native wrapper and `WindowApi::scroll`
   background-first dispatch for wheel, with tests for policy ordering and
   result metadata.
3. NetEase integration slice: move sidebar scroll to `WindowApi::scroll` and
   record actual selected path/attempts in artifacts.
4. AX scroll slice: implement AX scrollbar discovery/action/value mutation with
   tests and remove the relevant `TODO(background-scroll-ax)` markers. This is
   intentionally not part of the first implementation slice.
5. Optional keyboard-scroll slice: enable only with app-specific evidence and
   trace metadata showing it is a candidate path.

## Remaining Follow-Up

The trace field that should own scroll verification evidence is still a
follow-up decision for the future migration from product-local NetEase
interaction events to durable run storage.
