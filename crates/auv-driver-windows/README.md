# auv-driver-windows

Windows desktop driver for AUV. Exposes the same capability-oriented session
surface as `auv-driver-macos` and `auv-driver-linux`, adapted for Win32,
UIA, and `Windows.Media.Ocr`.

## Capabilities

- Display list / capture / region capture
- Window list, resolve, capture (`GDI PrintWindow`)
- Explicit WGC window/display images (`WindowApi::capture_wgc` / `DisplayApi::capture_wgc`): continuously drained latest-received frames, with [isolated native image-path evidence](../../docs/ai/references/driver/2026-10-30-wgc-on-demand-freshness.md).
- Window mutation: move, resize, set-frame, minimize, restore, zoom
- Window-targeted click/scroll: `ForegroundPreferred` uses `SendInput`; `BackgroundOnly`/`BackgroundPreferred` post `WM_LBUTTONDOWN`/`WM_LBUTTONUP`/`WM_LBUTTONDBLCLK`/`WM_MOUSEWHEEL` directly to the hit-tested control (`src/background_input.rs`), without raising or focusing the window — best-effort for classic Win32/MFC/WinForms controls, not for Chromium/Electron/WinUI surfaces
- Global click, scroll, type text, press key, copy, paste (`SendInput`)
- Clipboard snapshot / restore / set-text (text-only), plus rich (format-preserving) snapshot / restore covering every memory-backed clipboard format
- OCR via `Windows.Media.Ocr`
- Window-scoped OCR polling (`find_text` / `wait_text`)
- UIA accessibility tree capture, node focus, node select
- Process-level permission probe (elevation, UIAccess, interactive session)
- Readiness assessment: combines the permission probe with window presence, frontmost, frame-drift, and input-injection-target checks
- App-level activation by process image name (`ApplicationApi::activate`, served as `auv.api.driver.v1.ApplicationService/ActivateApplication` and `auv invoke app.activate`)
- Overlay visual adapter (cursor, outline, status layers) via `auv-driver-overlay-windows`, drawn with Direct2D/DirectWrite (real alpha, antialiasing, SVG cursor art, cursor shadows blurred from the art's silhouette)
- Background session (`WindowsDriverSession::background`): up to five target windows operated and watched without ever foregrounding them. Window-message click and wheel only (no input policy, no foreground route), cached targets revalidated by one handle probe per operation, typed `done` / `skipped_minimized` / `target_lost` / `unsupported` / `failed` answers, and a continuously drained per-window stream that retains the latest received frame ([reference](../../docs/ai/references/driver/2026-10-10-windows-background-session.md))
- Live overlay that follows delivered input (`OverlayApi::follow_operations`, `overlay` feature): one cursor per window in its own color that springs to each point, tilting as it moves, click ripples and window marks at 60 fps, drawn from what the driver reports and never sending input, plus a caller-written status beside each window's cursor (`OperationFollower::set_status`) ([reference](../../docs/ai/references/driver/2026-10-10-windows-overlay-motion.md))

## Open TODOs

| Marker | Location | What is deferred |
|---|---|---|
| `TODO(windows-input-target-lease)` | `src/input.rs` | Typed input lease matching the macOS slice |
| `TODO(windows-window-mutation-fallback)` | `src/mutation.rs` | Foreground/SendInput fallback for mutation when the Win32 API is blocked |
| `TODO(windows-window-zoom-verification)` | `src/mutation.rs` | Zoom/maximize state verification after mutation |
| `TODO(windows-window-capture-dpi)` | `src/capture.rs` | Tighter DPI/border mapping for window capture |
| `TODO(windows-ax-value-write)` | `src/accessibility.rs` | UIA `ValuePattern` writes |
| `TODO(windows-driver)` | `src/descriptor.rs` | Extend capability strings as slices land |
| `TODO(driver-overlay-windows-outline-label)` | `crates/auv-driver-overlay-windows/src/window.rs` | Outline label layout differs from macOS |
| `TODO(overlay-follow-other-input)` | `src/overlay_follow.rs` | Scroll, key, text and held-button delivery do not report to the live overlay |
| `TODO(overlay-follow-remote-runner)` | `src/overlay_follow.rs` | Events are reported in the process that delivers input; a remote Runner needs the same hook |
| `TODO(overlay-follow-multi-mouse)` | `src/session.rs` | Only the shared default mouse drives the live cursors |
| `TODO(overlay-live-status-producers)` | `src/overlay_follow.rs` | Only Rust callers can set a cursor status; `auv invoke`, MCP and the Runner do not start a follower |
| `TODO(windows-targeted-keyboard)` | `src/input.rs`, `src/background_session.rs` | Background keyboard delivery; a session answers keyboard requests as `unsupported` |
| `TODO(windows-posted-minimized)` | `src/background_input.rs` | Direct posted clicks and wheels do not refuse a minimized target under `background_only` |
| `TODO(windows-background-cloaked)` | `src/background_session.rs` | A window DWM cloaks (another virtual desktop) is treated as live; it needs its own typed state and live evidence |
| `TODO(background-session-wait-for-frame)` | `src/background_session.rs` | A blocking wait for the next frame; callers poll `capture`, which answers `Unchanged` without copying |
| `TODO(background-session-foreground-grant)` | `src/background_session.rs` | A delivery reports no focus disturbance without observing it; a just-launched process can still take the foreground on its first activation |
| `TODO(wgc-health-latest-frame)` | `src/wgc.rs` | The separate frozen health sampler still polls a FIFO and wake tokens; it needs its own health-baseline slice |
| `TODO(wgc-capture-identity)` | `src/wgc.rs` | Extend the existing handle/size image-cache key with PID and typed window/display identity receipts in a capture-identity slice |
| `TODO(driver-overlay-windows-window-owner-thread)` | `crates/auv-driver-overlay-windows/src/animator.rs` | A running animator must be the only presenter in its process |

## Architecture notes

Global (non-window-targeted) input delivery uses `SendInput` exclusively, and every
foreground `WindowInput` path foregrounds the target window first
(`SetForegroundWindow`/`ShowWindow`), mirroring how `auv-driver-linux` handles the
RemoteDesktop portal.

Window-targeted `BackgroundOnly`/`BackgroundPreferred` clicks and scrolls instead go
through `src/background_input.rs`, which posts window messages
(`WM_LBUTTONDOWN`/`WM_LBUTTONUP`/`WM_LBUTTONDBLCLK`/`WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL`) directly to the
control hit-tested under the target point via `PostMessageW`, after descending the
UIA-adjacent Win32 child hierarchy with `ChildWindowFromPointEx`. This never raises,
focuses, or activates the window, unlike `SendInput`. Delivery is best-effort:
`PostMessageW` succeeding only means the message was queued, not that the target
processed it. A posted message carries only what is in it (no cursor, focus or key
state), so whether an application acts on it varies per application: classic Win32
controls do, XAML content ignores posted mouse input, and a Chromium page counts
posted clicks but did not scroll on a posted wheel while covered (see the support
matrix's "Windows Background Session By Application" table). Windows has no second, more
compatible route today, so `ClickOptions::window_strategy`'s two variants
(`ChromiumCompatible`/`PidTargeted`) both resolve to this same delivery on Windows.

A background session (`src/background_session.rs`) is the contract a long-running agent uses on top
of those routes. It takes no input policy, because the only routes it has are posted window messages
and Windows.Graphics.Capture: nothing in it activates, restores, raises or resizes a window, moves the
cursor, or sends global input, and a request that has no such route (keyboard input, Alt/Meta clicks)
is answered `unsupported` instead of being sent to whatever has the foreground. All OS access goes
through one small `Backend` trait, so routing, follow-on-move, minimized skipping, lost-target isolation
and the no-fallback behavior are unit tested without a desktop. Captured frames use `FrameStream`
(`src/wgc_stream.rs`). Both per-window background streams and the one-target on-demand WGC image cache
drain their pool when frames arrive and retain the latest received GPU texture. Reads do not prove
that a pending application action has painted. Whether a posted click makes the
target take the *foreground* is decided by the operating system from the target process, not by this
driver: an independently launched application that handles a posted click activates itself inside its
own input queue and the foreground stays with the user, but a process that holds the right to take the
foreground (for example one started by the foreground application) takes it. See the reference note
for the measurements.

`ClipboardApi::snapshot_rich`/`restore_rich` (`src/clipboard.rs`) enumerate every
clipboard format present via `EnumClipboardFormats` and capture the raw bytes behind
each memory-backed (`HGLOBAL`) format, so a restore replaces exactly what was there
(files, images, HTML/RTF, and other registered formats), not just text. GDI
object-backed formats (`CF_BITMAP`, `CF_METAFILEPICT`, `CF_PALETTE`,
`CF_ENHMETAFILE`) are skipped, since duplicating those handles needs GDI object
duplication rather than a memory copy. Per-format and total snapshot bytes are
capped (64 MiB / 256 MiB) so a huge payload fails with a clear error instead of
an unbounded copy. `snapshot`/`restore`/`set_text` stay text-only, mirroring the
macOS driver's clipboard contract.

The overlay renders via a full-virtual-screen layered Win32 window. Each `present`
call draws all layers onto an off-screen 32-bpp DIB section through a Direct2D DC
render target (premultiplied alpha, antialiasing, DirectWrite labels) and presents the
frame via `UpdateLayeredWindow`. The live overlay (`Animator`) calls `present` once per
frame, at most 60 times a second, with the layers `MotionScene` composes for that instant;
the drawing code is the same one-shot path.

Readiness assessment (`assess_readiness`) treats `interactive_session = Missing` as
the only hard blocker. Elevation and UIAccess are diagnostic only — a
non-elevated process without UIAccess is the normal automation posture.

App-level activation resolves the process's most relevant window via the
`main_visible` selector (foreground → titled → largest), foregrounds it, then
verifies by re-observing the foreground window's owning process name.
