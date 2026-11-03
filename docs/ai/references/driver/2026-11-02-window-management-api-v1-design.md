# Window Management API V1 Design

Status: **approved by the owner on 2026-11-03.** RPC names follow the
[naming normalization](../session-api/2026-11-02-driver-api-naming-normalization.md),
which lands first. It extends
[`2026-06-05-window-management-api-v0.md`](2026-06-05-window-management-api-v0.md).

## Problem

AUV can list windows on macOS, Windows and Linux. The macOS and Windows
drivers can also move, resize, minimize, restore and zoom a window. But:

1. The mutation methods are not reachable. No Proto RPC, Runner service,
   Rust client, invoke command, MCP tool or JS SDK method calls them. Only
   `crates/auv-driver-windows/examples/validate.rs` does.
2. The read model is thin. `Window` has `frame`, `is_main` and `is_visible`.
   It has no minimized, maximized or fullscreen state, no stacking order and no
   virtual desktop. The listings also hide windows: macOS lists only on-screen
   windows (`.optionOnScreenOnly`), so minimized windows and windows on other
   Spaces are missing; Windows drops cloaked windows (other virtual desktops)
   and lists a minimized window at `(-32000, -32000)` with `is_visible: true`
   (`TODO(windows-window-list-minimized)`).
3. Nothing can subscribe to window changes on any platform.
4. Linux (GNOME Wayland) cannot mutate or activate windows at all
   (`TODO(wayland-app-activate)`).
5. Names are not uniform: the driver says `move_to`, `resize`, `restore`,
   `zoom`; the only activation is application-level `app.activate`.

## Goals

- One name per operation, the same in the driver, Proto, invoke/MCP and the JS
  SDK. Names follow Tauri v2 where Tauri has an equivalent.
- A read model that can answer: where is the window, how big, is it
  minimized/maximized/fullscreen, which desktop is it on, and what is in front
  of it.
- A subscription that follows the agreed `pointer-watch` rules
  ([`2026-10-09-pointer-position.md`](2026-10-09-pointer-position.md)).
- A JS surface with Tauri-style listeners and an optional alien-signals
  adapter.
- macOS and Windows. Linux keeps its current read path and returns
  `Unsupported` for the new methods.

## Non-goals (this design)

- Window drag, pointer-based repositioning or the foreground mutation fallback
  (still `TODO(windows-window-mutation-fallback)` and the v0 deferral).
- Setting always-on-top, or moving a window to another virtual desktop. See
  [Deferred](#deferred).
- Close, hide/show. `hide` is not minimize: on macOS it hides the whole
  application, on Windows `SW_HIDE` removes the window from the taskbar. Both
  are deferred.
- Inner (content-area) geometry. `frame` stays the outer frame.

## Naming

RPC names follow the agreed rules in
[`2026-11-02-driver-api-naming-normalization.md`](../session-api/2026-11-02-driver-api-naming-normalization.md):
AIP verb + noun in Proto, and an operation id derived from the RPC.

References (checked 2026-11-02):

- W3C WebDriver: `Get Window Rect`, `Set Window Rect`, `Maximize Window`,
  `Minimize Window`, `Fullscreen Window`. There is no restore command.
- Appium mac2: one `macos: adjustWindow` with `state` =
  `normal|minimized|maximized|fullscreen` and optional `x/y/width/height`.
  `normal` restores. WebDriver BiDi: `browser.setClientWindowState`.
- Appium NovaWindows: `windows: setFocus`. BetterGI (internal):
  `FocusWindow`, `ActivateWindow`.
- Tauri v2: `setPosition`, `setSize`, `minimize`/`unminimize`,
  `maximize`/`unmaximize`, `setFocus`, `onMoved`, `onResized`. Tauri has no
  window listing, stacking order or desktop id: it manages only its own windows.

Mutation is two RPCs, geometry and state, like mac2 and BiDi. A target state
replaces the minimize/unminimize/maximize/unmaximize pairs, so "what does
unminimize return to" needs no definition: the caller names the state.

| Operation | Proto RPC (`WindowService`) | Operation id | Driver (`WindowApi`) | JS `WindowClient` |
| --- | --- | --- | --- | --- |
| List | `ListWindows` (exists) | `window.list` | `list` | `windows.list()` |
| Read one, fresh | `GetWindow` | `window.get` | `get` | `refresh()` |
| Move and/or resize | `SetWindowFrame` (origin and size each optional) | `window.setFrame` | `set_frame` (replaces `move_to`, `resize`, `set_frame`) | `setFrame()`, plus `setPosition()` / `setSize()` sugar |
| Change state | `SetWindowState` (`NORMAL`, `MINIMIZED`, `MAXIMIZED`) | `window.setState` | `set_state` (replaces `minimize`, `restore`, `zoom`) | `setState()`, plus `minimize()` / `unminimize()` / `maximize()` / `unmaximize()` sugar |
| Focus | `FocusWindow` | `window.focus` | `focus` (Windows has `activate`) | `setFocus()` |
| Subscribe | `WatchWindows` (server stream) | `window.watch` (NDJSON stream) | `watch` | `watch()`, `onMoved`, `onResized`, `onStateChanged`, `onFocusChanged`, `onClosed` |

`WindowMutationKind` becomes `SetFrame { origin: Option<Point>, size:
Option<Size> }` and `SetState(WindowStateTarget)`. `WindowMutationPath`,
`WindowMutationOptions` and `WindowMutationResult` keep their names.

Notes:

- `SetWindowFrame` with only an origin is a move; with only a size it is a
  resize. One call that changes both avoids an intermediate frame that an app
  may clamp.
- `NORMAL` leaves both minimized and maximized (and fullscreen). The JS
  `unminimize()` sugar reads the state before the minimize and restores it, so
  it keeps Tauri's meaning; the wire stays a plain target state.
- `focus` vs `activate`: `app.activate` stays application-level. `window.focus`
  raises one window and gives it keyboard focus. Its result reports focus
  disturbance and the window that is foreground after settle, as
  `app.activate` does.

## Read model

### `Window` additions (`auv-driver-common/src/window.rs`)

`Window` gains one field, `state: WindowState`. `WindowState` grows from two
fields to:

```rust
pub struct WindowState {
  pub is_minimized: Option<bool>,
  pub is_maximized: Option<bool>,
  pub is_fullscreen: Option<bool>,
  pub is_visible: Option<bool>,
  /// Has keyboard focus. Distinct from `Window::is_main`, which keeps its
  /// selector meaning (the app's main window).
  pub is_focused: Option<bool>,
  pub is_always_on_top: Option<bool>,
  /// 0 is the frontmost window in this listing. Valid only inside the listing
  /// or event that carries it; not a stable identity.
  pub stack_index: Option<u32>,
  pub desktop: Option<WindowDesktop>,
}

pub struct WindowDesktop {
  /// Opaque platform id: a Windows virtual desktop GUID, a GNOME workspace
  /// index. Callers compare it; they do not parse it.
  pub id: String,
  pub is_current: bool,
}
```

`None` means "this platform cannot tell", never "false". Each `None` that a
platform returns gets a `TODO:` marker at its producer.

`WindowMutationResult` keeps `before_state`/`after_state`, now with the full
`WindowState`.

### `frame` semantics

- `frame` stays the outer frame in logical screen coordinates.
- For a minimized window, `frame` is the frame it returns to (Windows
  `GetWindowPlacement.rcNormalPosition`, macOS AX position/size). It is never
  `(-32000, -32000)`.
- Input and capture keep refusing a minimized window. They check
  `state.is_minimized`, not the frame.

### Listing scope

`ListWindowsRequest` gains filters. Default behaviour changes to list
everything a user could switch to:

```proto
message ListWindowsRequest {
  reserved 1;
  reserved "lease";
  // Default false: minimized windows are listed.
  bool exclude_minimized = 2;
  // Default false: windows on other virtual desktops are listed.
  bool exclude_other_desktops = 3;
}
```

This follows the existing rule for read APIs: include by default, opt out with
a flag.

### Main window selection

`WindowSelector.main_visible` (and every caller that needs "the app's main
window", such as `app.activate`) uses one shared narrowing cascade in
`auv-driver-common`, decided with the owner on 2026-11-03. The candidates are
the windows that match the selector's application and title. Each step keeps
only the candidates that pass it, **but is skipped when it would leave none**.
Selection stops as soon as one candidate is left.

1. One candidate: take it, even if it is minimized or on another desktop.
2. Prefer windows on the current desktop (`desktop.is_current != Some(false)`).
3. Prefer windows that are not minimized (`is_minimized != Some(true)`).
4. Prefer the focused window (`is_focused == Some(true)`).
5. Prefer the platform's main window (`is_main`).
6. Prefer windows with a non-empty title.
7. Take the frontmost (`stack_index`, lowest first), then the largest frame,
   then the first in listing order.

So a single minimized window is still found, several windows prefer a shown
one, and several minimized windows fall back to the frontmost of them.
`window.list` still reports every window with its state, so a caller can see
why one was chosen.

The cascade is `select_main_window` in `auv-driver-common/src/window.rs`.
On macOS, `resolve(main_visible)` runs it in two phases to keep the AX cost
off the common path: first over the on-screen listing (no AX), which is final
when it yields a normal window, since a shown window always beats a minimized
one; only when the app shows no normal window does it list that one app's
windows with AX state (minimized included) and run the cascade again. Lookups
by id or title and `click_target` stay on the on-screen listing.

Platforms may run a pre-filter before the cascade for surfaces that are never a
main window (the Linux desktop shell surface). `NOTICE(linux-window-selection)`:
Linux keeps its existing selection heuristics unchanged (owner decision), and
its state fields are mostly `None`, so steps 2–4 and 7 do not narrow there.

### Per platform

| Field | macOS | Windows | Linux (this design) |
| --- | --- | --- | --- |
| Listing | `CGWindowListCopyWindowInfo(.optionAll)` filtered to layer 0 app windows, joined with AX windows per pid | `EnumWindows` (already Z order), keep cloaked windows | AT-SPI, unchanged (origin repaired from the Shell stage by size match) |
| `is_minimized` | `kAXMinimizedAttribute` | `IsIconic` | `None` |
| `is_maximized` | No maximized state. Frame equals the `visibleFrame` of the window's display (the display under the window's center, else the largest overlap) within 2 pt, or the frame an AUV maximize settled at when the app constrained it (`NOTICE(macos-window-maximized)`) | `IsZoomed` | `None` |
| `is_fullscreen` | `AXFullScreen` attribute | Frame covers the monitor and no caption style. Best effort | `None` |
| `is_focused` | Frontmost app + `kAXFocusedWindow` | `GetForegroundWindow` | AT-SPI `ACTIVE` (already used for `is_main`) |
| `is_always_on_top` | `kCGWindowLayer > 0` | `WS_EX_TOPMOST` | `None` |
| `stack_index` | CGWindowList order (front to back) | `EnumWindows` order | `None` |
| `desktop` | `None`: Spaces need private CGS API. `TODO(macos-window-space)` | `IVirtualDesktopManager::GetWindowDesktopId` / `IsWindowOnCurrentVirtualDesktop` (public) | `None` |

Risk: macOS AX reads per window are slower than CGWindowList. Measure
`window.list` latency before and after, as
[`2026-10-20-linux-window-list-latency.md`](2026-10-20-linux-window-list-latency.md)
did for Linux. If it regresses, read AX state only for the windows the request
needs.

## Mutation

The driver already has the mechanics for frame, minimize, restore and zoom.
This slice adds:

- **`SetWindowState` per platform.**

  | Target | macOS | Windows | Linux |
  | --- | --- | --- | --- |
  | `NORMAL` | `AXMinimized = false`; then, if maximized, set the frame recorded by this Runner's `MAXIMIZED` and drop the record. Without a record (maximized by hand, or a Runner restart) the frame stays and the attempt says so (`NOTICE(macos-maximize-restore-frame)`) | `SW_RESTORE` until neither `IsIconic` nor `IsZoomed` | `Unsupported` |
  | `MINIMIZED` | `AXMinimized = true` | `SW_MINIMIZE` | `Unsupported` |
  | `MAXIMIZED` | `AXMinimized = false` if minimized; record the current frame in Runner memory; set the frame to the `visibleFrame` of the window's display through the `SetWindowFrame` AX path. No zoom button (`AXZoomWindow` zooms to an app-defined frame). An app that constrains the frame gives `adjusted_by_app` | `SW_MAXIMIZE` | `Unsupported` |

  A request for the state the window is already in succeeds without a call
  and reports the unchanged state.
- **`SetWindowFrame` per platform.** Prior art: trycua/cua
  `libs/cua-driver/rust/crates/platform-{macos,windows}/src/tools/set_window_frame.rs`
  (checked 2026-11-03).
  - **Precondition (both platforms):** a minimized or full-screen window
    fails with `FAILED_PRECONDITION` and a message to call `setState(NORMAL)`
    first (macOS rejects moves of full-screen windows). A maximized window is
    allowed: it becomes a normal window at the requested frame, and
    `after_state.is_maximized` is `false` (owner decision 2026-11-03, see
    Decided). On Windows this is one `SetWindowPlacement` with
    `SW_SHOWNOACTIVATE` and `rcNormalPosition` set to the requested outer
    rect in workspace coordinates, so the window does not first restore to
    its old frame and is not activated. Missing origin or size parts come
    from the restore placement. `SetWindowPos` alone would move a maximized
    window but keep the maximized style.
  - **macOS (AX):**
    - Write `AXPosition` before `AXSize`. On macOS Tahoe, writing
      `AXPosition` right after `AXSize` can silently restore the old size
      while both writes report success.
    - Skip `AXSize` when the size does not change, so windows without a
      settable size (Calculator) can still move.
    - Retry up to 6 times, rewriting only the component that has not settled,
      so the two writes do not undo each other.
    - Confirm against the WindowServer frame (`CGWindowList`, the same source
      as `ListWindows`), not against the AX values just written.
    - Set `AXUIElementSetMessagingTimeout` (about 2 s) on the application and
      window elements, so a hung app cannot stall the call.
    - Refuse windows of the Runner's own process (AX writes to the calling
      process can deadlock).
  - **Windows (`SetWindowPos`):**
    - `ListWindows` reports the visible frame (`DWMWA_EXTENDED_FRAME_BOUNDS`),
      but `SetWindowPos` takes the outer rect (`GetWindowRect`), which on
      Windows 10/11 includes an invisible resize border of about 7 px on the
      left, right and bottom. Convert the requested visible frame to the outer
      rect with the current difference between the two rects before calling
      `SetWindowPos`. The current `mutation.rs` does not, so `move_to`,
      `resize` and `set_frame` likely land off by the border and fail the 2 px
      verification. Reproduce this on luoling-windows-11 with a test first
      (cua has `compensates_for_invisible_resize_borders`).
    - Keep `SWP_NOACTIVATE | SWP_NOZORDER`. Read back up to 6 times, 40 ms
      apart.
    - `REVIEW:` confirm the DPI space of `SetWindowPos` coordinates for the
      Runner process, and that a move to a display with another scale
      (`WM_DPICHANGED` resizes the window) is reported, not failed.
  - **App-imposed limits:** a window that moved but settled at another size
    (minimum or maximum size, terminal grid snapping) succeeds with the actual
    frame and an `adjusted_by_app` flag. Only a window that did not change at
    all is a verification failure.
- **Verification** reads the state back after settle: `IsIconic` / `IsZoomed`
  on Windows (removes `TODO(windows-window-zoom-verification)`), AX
  attributes on macOS. macOS `MAXIMIZED` and the `NORMAL` restore are
  checked like `SetWindowFrame`: a window that did not move fails, and a
  frame the app constrained is `adjusted_by_app`.
- **`focus`.** Windows reuses `activate_window`. macOS reuses
  `activate_app_for_window` plus `kAXRaiseAction`. Linux returns `Unsupported`
  (see [Linux](#linux-not-supported-yet)).
- **Failure layers** stay as they are: a stale window id is `NotFound`, an
  unsupported platform or target is `Unsupported` (gRPC `UNIMPLEMENTED`), a
  failed check after the call is a verification failure that carries the
  attempts.

Tracing: the Runner's gRPC recording layer (`auv-cli/src/runner/recording.rs`)
already records every unary call and its response. The mutation responses carry
`WindowMutationResult`, so no extra artifact type is needed.

## Subscription

`WatchWindows` follows the AUV watch convention (see "Watch" in
`docs/TERMS_AND_CONCEPTS.md`): snapshot, then changes, with a resume token.
It refines `TODO(pointer-watch)` by adding resume. References: Firestore
Listen (`resume_token`, `RESET`, `CURRENT`), Kubernetes watch
(`resourceVersion`, bookmarks, `410 Gone`), MongoDB change streams
(`resumeAfter`), SSE `Last-Event-ID`, Phoenix Presence (`presence_state` then
`presence_diff`).

### Wire contract

```proto
rpc WatchWindows(WatchWindowsRequest) returns (stream WatchWindowsResponse);

message WatchWindowsRequest {
  // Empty: every window. Otherwise windows, application selector,
  // exclude_minimized, exclude_other_desktops, as for ListWindows.
  WindowFilter filter = 1;
  // The latest token from an earlier stream. Empty: a new subscription.
  string resume_token = 2;
}

message WatchWindowsResponse {
  // Position after this message. Every message carries one.
  string resume_token = 1;
  // When the Runner read the state this message reports.
  google.protobuf.Timestamp sampled_at = 2;
  oneof event {
    WindowsSnapshot snapshot = 3;  // full state; the client replaces its copy
    WindowAdded added = 4;         // a window entered the filtered set
    WindowModified modified = 5;   // full Window plus changed_fields
    WindowRemoved removed = 6;     // closed, or no longer matches the filter
    WindowsCurrent current = 7;    // the client now holds the current state
    WatchHeartbeat heartbeat = 8;  // nothing changed; the token advanced
  }
}

message WindowsSnapshot {
  repeated Window windows = 1;
  SnapshotReason reason = 2;  // INITIAL (no token) or RESET (token not resumable)
}

message WindowModified {
  Window window = 1;
  repeated WindowField changed_fields = 2;  // POSITION, SIZE, STATE, FOCUS, STACK, DESKTOP, TITLE
}

message WindowRemoved {
  WindowRef window = 1;
  WindowRemovedReason reason = 2;  // CLOSED or FILTERED_OUT
}
```

### Stream rules

1. **Start.** No token: `snapshot(INITIAL)`, `current`, then changes.
2. **Resume.** A token the Runner can resume: the net difference between the
   state the old stream last sent and the current state, as `added` /
   `modified` / `removed`, then `current`, then changes. A token it cannot
   resume (expired, Runner restarted, other filter, other caller, already
   used, malformed): `snapshot(RESET)`, `current`, then changes. An unusable
   token is never an error, so the client has one path.
3. **Net difference, not replay.** The Runner keeps no event log. A window that
   moved twice while the client was away arrives as one `modified`. Watch is
   state sync; the full event history belongs to trace recording.
4. **Opaque token.** Clients store and return it unchanged. Inside: a version,
   a 128-bit random stream id, the sequence, the Runner instance id (new on
   every start), and a filter fingerprint. The encoding may change.
5. **Bound to the caller.** A resume from a different Device or Run identity
   than the original stream is treated as unusable and answered with
   `RESET`, without revealing whether the stream exists.
6. **Store after apply.** A client stores a token only after it applied that
   message. Every change carries the full `Window`, so applying a message
   twice gives the same state; delivery is at least once.
7. **Heartbeat.** With no change for 15 s the Runner sends `heartbeat`, so the
   token stays fresh and a dead connection shows up.
8. **Retention.** After a stream ends, the Runner keeps its last sent state
   for 30 s, capped per Runner (oldest dropped first). A token resumes once;
   the resumed stream continues from it. Nothing survives a Runner restart.
9. **Backpressure.** Each subscriber has at most one pending event per window,
   merged in place: `modified`+`modified` keeps the latest window and the
   union of `changed_fields`; `added`+`modified` stays `added`;
   `added`+`removed` cancels; `modified`+`removed` becomes `removed`. Nothing
   is dropped, and the pending set is bounded by the window count. If it
   still overflows, the stream ends with `RESOURCE_EXHAUSTED`; the client
   resumes with its token.
10. **Cancel.** Cancelling the RPC ends the stream and starts its retention.

`TODO(watch-resume)`: the first version may answer every token with
`snapshot(RESET)`. That is correct behavior; precise resume (rule 2, first
case) and retention (rule 8) follow. The wire contract is final from the
first version.

### Alternatives considered

Decided with the owner on 2026-11-02 and 2026-11-03.

- **No resume; a reconnect relists.** The first proposal followed
  `TODO(pointer-watch)` ("no replay or resume in the first version"). Rejected:
  without a token the client cannot tell what changed while it was away, so
  `onClosed` and similar listeners would miss windows that closed during the
  gap unless every client diffed by hand. The resume token keeps that
  diffing in one place.
- **Initial state as one `added` per window, then a `synced` marker**
  (Kubernetes informer style, and the `WatchRunners` sketch in
  `2026-07-31-device-run-runner-aggregated-api-design.md`). Rejected in favor
  of one `snapshot` message: a single message is an atomic replace for the
  client, and the same message doubles as the `RESET` answer to a token that
  cannot resume, so clients have one code path. Changes still use the
  `added` / `modified` / `removed` vocabulary.
- **An error for an unusable token** (Kubernetes `410 Gone`, MongoDB's error
  after oplog rollover). Rejected in favor of a `snapshot(RESET)` event
  (Firestore `RESET`): the client never needs an error branch that relists
  and watches again.
- **Replaying an event log on resume** (MongoDB change streams, etcd
  revisions). Rejected: the Runner would have to keep a bounded log with
  compaction rules. Windows are OS state, so the net difference between the
  last state sent and the current state is enough for state sync; history
  belongs to trace recording (rule 3).
- **A resource name or sequence number as the token.** Rejected for an
  opaque token (Kubernetes `resourceVersion`, AIP-158 page tokens): the
  Runner can change what it encodes, and the token can carry the Runner
  instance id, filter fingerprint and caller binding that make "cannot
  resume" detectable without a lookup (rules 4 and 5).
- **Partial updates** (only the changed fields). Rejected: every change
  carries the full `Window` plus `changed_fields`, so applying a message twice
  is harmless (rule 6) and listeners such as `onMoved` need no client-side
  diff.
- **Drop events under backpressure.** Rejected: lifecycle events must not be
  lost. Merging pending events per window bounds the backlog by the number of
  windows instead (rule 9).
- **A cache before the watch.** Per-pid app info caching and parallel AX
  reads were considered for the slow macOS `list()`; deferred so that the
  watcher's model becomes the one window cache (`TODO(window-list-cache)`).
- **JS reactivity in the SDK core.** Rejected: alien-signals lives in the
  `@auv-js/sdk/signals` subpath with an optional peer dependency, driven by
  the async iterator; the core SDK exposes `watch()` and Tauri-style `on*`
  listeners only.

### Driver side

```text
platform notification (AXObserver / WinEventHook) --+
periodic full resync (safety net, about 2 s) -------+--> re-read affected windows
polling (platforms without notifications) ----------+    -> diff with last state -> events
```

- Notifications are hints. The driver re-reads the window and diffs it with
  the last known state, so watched state always matches `ListWindows` and a
  missed or reordered notification cannot corrupt it.
- `auv-driver-common/src/window_watch.rs` (new) owns the diff, the per-window
  pending merge (rule 9) and the polling source. Platforms supply only the
  notification source and the read.
- Each platform `WindowApi` gets `watch() -> DriverResult<WindowWatch>`.
  `WindowWatch` is a receiver plus a guard; dropping it stops the native
  source.
- Native sources:
  - **macOS**: `AXObserver` per application pid on one dedicated CFRunLoop
    thread in Swift (`WindowObserver.swift`, new). Notifications:
    `kAXWindowCreated`, `kAXUIElementDestroyed`, `kAXWindowMoved`,
    `kAXWindowResized`, `kAXWindowMiniaturized`, `kAXWindowDeminiaturized`,
    `kAXFocusedWindowChanged`, `kAXApplicationActivated`. `NSWorkspace`
    launch/terminate attaches and detaches observers. There is no stacking
    notification; stacking is re-read on focus changes and on resync.
  - **Windows**: `SetWinEventHook(WINEVENT_OUTOFCONTEXT)` on a dedicated thread
    with a message loop (`window_events.rs`, new). Events:
    `EVENT_OBJECT_CREATE/DESTROY/SHOW/HIDE/LOCATIONCHANGE` (filtered to
    `OBJID_WINDOW`), `EVENT_SYSTEM_MINIMIZESTART/END`,
    `EVENT_SYSTEM_FOREGROUND`, `EVENT_OBJECT_CLOAKED/UNCLOAKED`. The hook needs
    the interactive desktop. It works in the user daemon (`auv serve` in the
    console session). From an SSH session it fails, as `GetCursorPos` does in
    the pointer evidence.

### Runner side

- One driver watcher serves every subscriber. It starts with the first and
  stops with the last. Each subscriber has its own filter, pending set and
  retention entry.
- The watcher's window model doubles as the Runner's window cache. While a
  watcher runs, `ListWindows`, `GetWindow` and window-id lookups for input read
  the model instead of asking every app over AX (macOS) or re-enumerating.
  Mutation verification still reads fresh state.
  `TODO(window-list-cache)`: no other cache (per-pid app info, parallel AX
  reads) is added before this; decided with the owner on 2026-11-03.
- Events carry metadata only, never pixels (AGENTS.md "Image Payloads").
- Recording: the gRPC recording layer records every message today. For a
  watch stream it records start, end, resume outcome (`resumed` / `reset`),
  event counts and errors, not each event.

### Frontends

- **CLI**: `auv invoke window.watch` prints NDJSON until interrupted.
  `--resume-token` resumes.
- **MCP**: no watch tool; MCP tools are request/response.
  `TODO(window-wait-for-state)`: a bounded `window.waitForState` if an agent
  needs one.
- **JS SDK core** (`js/packages/sdk/src/apis/auv/driver.ts`, no new
  dependency):
  - `driver.windows.watch(options)` → `AsyncIterable` of raw messages. It
    reconnects with the latest token by itself.
  - `WindowClient.onMoved`, `onResized`, `onStateChanged`, `onFocusChanged`,
    `onClosed`, each returning `Promise<() => void>` like Tauri's `UnlistenFn`.
    Listeners with the same filter share one stream (reference counted).
  - On `snapshot`, the SDK diffs it with its cache and fires the matching
    listeners before it replaces the cache, so a `RESET` and a precise resume
    look the same to listeners.
- **alien-signals** (`@auv-js/sdk/signals` subpath, `alien-signals` as an
  optional peer dependency):
  - `windowsSignal(driver.windows, options)` →
    `{ windows: Signal<ReadonlyMap<string, Window>>, ready, error, dispose }`.
    `ready` turns true at `current`.
  - `windowSignal(client)` derives one window, with `frame` and `state` as
    `computed`.
  - During a reconnect the old values stay readable and `ready` stays true;
    a `RESET` snapshot replaces the map in one update.

### Order of work

1. `window_watch.rs`: diff, pending merge, polling source, unit tests.
2. Runner stream: shared watcher, filters, `snapshot(RESET)` for every token,
   heartbeat, recording summary.
3. CLI NDJSON, JS `watch()` and `on*`.
4. `@auv-js/sdk/signals`.
5. Precise resume and retention.
6. Native sources (AXObserver, WinEventHook); polling becomes the resync.

## Module placement

| Module | Change | Solves |
| --- | --- | --- |
| `crates/auv-driver-common/src/window.rs` | `WindowState` fields, `WindowDesktop`, `WindowMutationKind::{SetFrame, SetState}` | One read and mutation contract for every platform |
| `crates/auv-driver-common/src/window_watch.rs` (new) | Change types, coalescing queue, polling source | Shared subscription rules; works before native hooks |
| `crates/auv-driver-macos/native/swift/.../Window.swift` | List all windows, read AX state | Minimized and fullscreen windows visible in the list |
| `crates/auv-driver-macos/native/swift/.../WindowObserver.swift` (new) + `src/native/binding.rs` | AXObserver thread | Native macOS events |
| `crates/auv-driver-macos/src/session.rs` | `set_frame`, `set_state`, `focus`, `watch` | Uniform driver API |
| `crates/auv-driver-windows/src/window.rs` | State fields, placement frame, desktop id, keep cloaked windows | Correct minimized frame and desktop |
| `crates/auv-driver-windows/src/mutation.rs` | `set_state` with read-back verification | Verified results |
| `crates/auv-driver-windows/src/window_events.rs` (new) | WinEventHook thread | Native Windows events |
| `crates/auv-driver-linux/src/session.rs` | Same method names; new mutations and `watch` return `Unsupported` with the PR-welcome message | `LocalDriverSession` compiles on Linux |
| `proto/auv/api/driver/v1/window.proto` | New RPCs and messages, `ListWindowsRequest` filters | One wire contract for CLI, MCP, SDK |
| `crates/auv/src/protocol/window.rs` (new) | Encode and decode window state and mutation results | Shared Proto mapping (like `protocol/application.rs`) |
| `crates/auv-cli/src/runner/local_driver.rs` | `LocalWindowService` methods | Runner executes the driver calls |
| `crates/auv/src/client/runner.rs` | Typed Rust client methods | Library callers and invoke use one path |
| `crates/auv-cli-invoke/src/commands/window.rs` | `window.get`, `setFrame`, `setState`, `focus`, `watch` | CLI and MCP (MCP tools come from the invoke registry) |
| `js/packages/sdk/src/apis/auv/driver.ts` | `WindowClient` methods, listeners | JS surface |
| `js/packages/sdk/src/signals/` (new, provisional) | alien-signals adapter | Reactive UI consumers |
| `docs/TERMS_AND_CONCEPTS.md` | "Window state", "Window change", "Window watch" | Shared vocabulary |

There is no shared `WindowApi` trait. `LocalDriverSession` derefs to the
platform session, so each platform keeps same-named inherent methods, as today.
This design does not add a trait.

## Linux (not supported yet)

The Linux driver gains the new methods with the same names, and every new
mutation or subscription returns `Unsupported` with a message that it is not
supported on Linux yet and that pull requests are welcome, for example:
`window.setFrame is not supported on Linux yet; pull requests are welcome at
https://github.com/moeru-ai/auv`. Each stub carries
`TODO(linux-window-management)` pointing to this section.

| Operation | Linux |
| --- | --- |
| `ListWindows`, `ResolveWindow` | Unchanged. The existing AT-SPI listing and its heuristics (Shell stage origin repair by size match, active-window `is_main`) stay as they are. |
| `GetWindow` | Supported: list and find, as `ListWindows`. |
| New `WindowState` fields | `None`, except `is_focused` from AT-SPI `ACTIVE`. |
| `ListWindowsRequest` filters | Accepted; `exclude_other_desktops` has no effect because `desktop` is `None`. |
| `SetWindowFrame`, `SetWindowState`, `FocusWindow` | `Unsupported` (not yet, PR welcome). |
| `WatchWindows` | `Unsupported` (not yet, PR welcome). |

Research notes for a future contributor (2026-11-03, not a plan):

- No Wayland protocol lets a client move another client's window. Per
  compositor, the known routes are: Sway and Hyprland IPC (read and write),
  niri IPC (read, activate, floating geometry), `wlr-foreign-toplevel-management`
  (state and activate, no geometry), KDE KWin scripts loaded over D-Bus at
  runtime (read and write; unverified), and on GNOME only a Shell extension.
  X11 and XWayland windows accept EWMH requests. AUV `main` has no X11 backend.
- GNOME without an extension reads only AT-SPI: origins are `(0, 0)` on Mutter,
  there is no stacking or workspace, and apps without accessibility are not
  listed. `org.gnome.Shell.Introspect.GetWindows` returns `AccessDenied` on
  neko-gpu-1 (GNOME Shell 48.7).
- Prior art: trycua/cua `libs/cua-driver/wayland-helper/winrects@cua` (GNOME
  45–49, read, verified activation, capture; refused on GNOME 50 after
  session crashes; needs a re-login) and `libs/cua-driver/kwin-target-helper`
  (read-only KWin effect). Both verify the D-Bus owner is the compositor
  process before trusting it. Neither moves or resizes windows.

## Slices

1. **Contract and read model.** `WindowState`, renamed mutation kinds, macOS
   and Windows listing changes, `ListWindowsRequest` filters, terms doc.
   Evidence: `window.list` on macOS and luoling-windows-11 with a minimized and
   a maximized window, and a window on a second Windows desktop.
2. **Mutations end to end.** Proto RPCs, Runner, Rust client, invoke/MCP, JS
   `WindowClient`. Evidence: each operation on macOS and Windows, with
   before/after state in the response.
3. **Watch** on macOS and Windows, in the order under
   [Subscription](#order-of-work). Linux returns `Unsupported`.

## Decided (2026-11-03)

- `window.list` lists minimized windows and windows on other desktops by
  default, with `exclude_minimized` / `exclude_other_desktops` opt-outs.
- macOS maximize = fill the display's visible frame; restore uses the
  recorded frame (owner's option A, 2026-11-03; replaces the zoom-button
  approach). The record lives in Runner memory only, so it pairs a maximize
  and a restore only when both go through the same Runner (SDK, MCP, or
  `auv --device <id> invoke`). A plain `auv invoke` without `--device` runs the
  direct local path inside the CLI process (checked 2026-11-03), so a restore
  in a later CLI call finds no record and leaves the frame. A maximized
  window counts as maximized when it fills its display's visible frame or
  sits at the frame AUV's own maximize ended at (an app may stop short).
- `SetWindowState` has no `FULLSCREEN` in this design
  (`TODO(window-fullscreen)`: macOS full screen moves the window to its own
  Space with an asynchronous animation, and Win32 has no generic full screen
  for another app's window). `is_fullscreen` stays on the read side.
- macOS Spaces (Mission Control desktops) are not handled: `desktop` is
  `None` on macOS. `TODO(macos-window-space)`: reading a window's Space needs
  the private CGS API; revisit only with an owner-approved need. Stage
  Manager grouping is not modeled either (`TODO(macos-stage-manager)`: no
  public API).
- Linux: new mutation and watch methods return `Unsupported` ("not supported
  on Linux yet; pull requests are welcome"). The existing AT-SPI listing and
  its heuristics stay unchanged. See [Linux](#linux-not-supported-yet).
- `set_frame` on a maximized window un-maximizes it to the requested frame on
  macOS and Windows; minimized is refused.

## Implementation notes (driver layer, 2026-11-03)

The driver contract and the three platform `WindowApi`s are in place. The
Runner, the Rust client, invoke, MCP and the JS SDK do not expose the new
calls yet; existing callers (`ListWindows`, `window.list`, `resolve_window_ref`)
keep working with the new `Window` shape. The frontend slice is in
[Implementation notes (API)](#implementation-notes-api-2026-11-04). Driver notes:

- `auv-driver-common`: `Window.state: WindowState` (all facts `Option`),
  `WindowDesktop`, `WindowStateTarget { Normal, Minimized, Maximized }`,
  `WindowMutationKind::{SetFrame { origin, size }, SetState { state }}`,
  `WindowMutationResult.adjusted_by_app`, `WindowListFilter`, and the
  `select_main_window` cascade, which the macOS and Windows resolvers and the
  Windows activation observation share.
- `DriverError::FailedPrecondition` is new (for example `set_frame` on a
  minimized window) and maps to invoke `failed_precondition` and Runner
  `FAILED_PRECONDITION`. `DriverError::Unsupported` carries an optional
  `recovery` (`DriverError::unsupported_with_recovery`).
- The former `move_to` / `resize` / `minimize` / `restore` / `zoom` methods
  are replaced by `set_frame`, `set_state` and `focus`.
- `resolve_window_ref` (the Runner input hot path) now calls `WindowApi::get`.
  Every platform's `get` is a full listing plus a lookup, so the cost is
  unchanged: macOS release build, 89 windows, 20 runs each: list+find
  26-27 ms, get 25-32 ms (noise). No cache (`TODO(window-list-cache)`).
- macOS: every state step polls the WindowServer frame until two reads 50 ms
  apart agree, for at most 1 s, because `after_frame` was read during the
  deminiaturize animation before (`NOTICE(macos-window-state-animation)`).
- macOS: `MAXIMIZED` used to press the green button, which enters full screen
  on current macOS, and then performed `AXZoomWindow`. Both are gone: maximize
  writes the visible frame (option A, see Decided) and keeps the restore
  record in the driver session's memory.
- macOS: the listing read the frontmost app from
  `NSWorkspace.frontmostApplication`, which stays stale in a process that does
  not pump the main run loop, so `focus` failed verification after a
  successful activation. It now reads the system-wide AX focused application
  (`NOTICE(macos-frontmost-stale)`).
- Windows: `set_frame` compensates the invisible resize border so the visible
  frame lands where requested; a maximized target is written with
  `SetWindowPlacement`. A minimized window is listed at its restore frame
  with `is_visible: false`. COM apartment setup is shared in `com.rs`.
- Linux: `get` is implemented; `set_frame`, `set_state` and `focus` return
  `Unsupported` with the pull-requests-welcome recovery. `is_focused` comes
  from the AT-SPI `ACTIVE` state.

## Implementation notes (API, 2026-11-04)

Mutations are wired end to end: `GetWindow`, `SetWindowFrame`,
`SetWindowState`, `FocusWindow` in `window_service.proto`; shared mapping in
`crates/auv/src/protocol/window.rs`; Runner `LocalWindowService`; Rust
`WindowClient::{refresh, set_frame, set_state, focus}` and
`WindowsClient::list_with`; invoke/MCP `window.get`, `window.setFrame`,
`window.setState`, `window.focus` (plus `window.list --exclude-minimized
--exclude-other-desktops`); JS `WindowClient` methods. Deviations and notes:

- `Window.state` is a nested `WindowState` message (all `optional`), reused
  by `WindowMutationResult.before_state`/`after_state`.
- The mutation messages live in `window_service.proto`, not `window.proto`:
  they carry `DisturbanceLevel` from `input.proto`, which imports
  `window.proto` (`NOTICE(window-mutation-messages)`). Sizes use a new
  `geometry.proto` `LogicalSize`.
- `SetWindowFrame`/`SetWindowState` keep `Set`: `proto/api-linter.yaml`
  disables `core::0134::synonyms` for `window_service.proto` with a reason.
- `WindowMutationOptions` are not on the wire; the Runner uses the driver
  defaults (`TODO(window-mutation-options)`).
- `window.setFrame` in invoke fills an omitted `--x`/`--y` (or
  `--width`/`--height`) from the current frame; the wire keeps origin and size
  as whole optional components.
- JS `unminimize()` restores the state recorded by the last `minimize()`
  through the same Runner client (maximized or normal), else `NORMAL`.
- Linux invoke cells are `Unsupported`, so a selected Linux Runner refuses
  before the RPC; the RPC itself answers `UNIMPLEMENTED` with the PR-welcome
  message.
- Scenario evidence:
  [`2026-11-03-window-management-v1-evidence.md`](2026-11-03-window-management-v1-evidence.md).
  Eval cases: `evals/auv-base` window-management tasks.

## Deferred

- Set always-on-top, move to another desktop: the read side
  lands first. Moving another process's window between Windows desktops needs
  the undocumented `IVirtualDesktopManagerInternal`.
- Close, hide and show.
- Inner (content-area) frame.
- Event replay for `WatchWindows` (rule 3: watch sends net differences).
