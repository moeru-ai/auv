# auv-driver-windows

Windows desktop driver for AUV. Exposes the same capability-oriented session
surface as `auv-driver-macos` and `auv-driver-linux`, adapted for Win32,
UIA, and `Windows.Media.Ocr`.

## Capabilities

- Display list / capture / region capture
- Window list, resolve, capture (`GDI PrintWindow`)
- Window mutation: move, resize, set-frame, minimize, restore, zoom
- Window-targeted click/scroll: `ForegroundPreferred` uses `SendInput`; `BackgroundOnly`/`BackgroundPreferred` post `WM_LBUTTONDOWN`/`WM_LBUTTONUP`/`WM_LBUTTONDBLCLK`/`WM_MOUSEWHEEL` directly to the hit-tested control (`src/background_input.rs`), without raising or focusing the window — best-effort for classic Win32/MFC/WinForms controls, not for Chromium/Electron/WinUI surfaces
- Global click, scroll, type text, press key, copy, paste (`SendInput`)
- Clipboard snapshot / restore / set-text (text-only), plus rich (format-preserving) snapshot / restore covering every memory-backed clipboard format
- OCR via `Windows.Media.Ocr`
- Window-scoped OCR polling (`find_text` / `wait_text`)
- UIA accessibility tree capture, node focus, node select
- Process-level permission probe (elevation, UIAccess, interactive session)
- Readiness assessment: combines the permission probe with window presence, frontmost, frame-drift, and input-injection-target checks
- App-level activation by process name (`ApplicationControl::activate_process_name`)
- Overlay visual adapter (cursor, outline, status layers) via `auv-driver-overlay-windows`

## Open TODOs

| Marker | Location | What is deferred |
|---|---|---|
| `TODO(windows-input-target-lease)` | `src/input.rs` | Typed input lease matching the macOS slice |
| `TODO(windows-window-mutation-fallback)` | `src/mutation.rs` | Foreground/SendInput fallback for mutation when the Win32 API is blocked |
| `TODO(windows-window-zoom-verification)` | `src/mutation.rs` | Zoom/maximize state verification after mutation |
| `TODO(windows-window-capture-dpi)` | `src/capture.rs` | Tighter DPI/border mapping for window capture |
| `TODO(windows-ax-value-write)` | `src/accessibility.rs` | UIA `ValuePattern` writes |
| `TODO(windows-driver)` | `src/descriptor.rs` | Extend capability strings as slices land |
| `TODO(app-activate-windows-cli)` | `crates/auv-cli-invoke/src/commands/app.rs` | Wire `activate_process_name` into the `app.activate` CLI command (needs an owner decision on the shared output contract) |
| `TODO(driver-overlay-windows-motion)` | `crates/auv-driver-overlay-windows/src/overlay.rs` | Per-layer position easing / animation |
| SVG cursor rasterization (see `NOTICE` in `src/window.rs`) | `crates/auv-driver-overlay-windows/src/window.rs` | `CursorImage::Svg` is rejected with `Err`; needs a rasterizer |

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
processed it — classic Win32/MFC/WinForms controls handle it reliably, while
Chromium/Electron/WinUI/UWP surfaces and most GPU-rendered custom UI read real HID
input instead and silently ignore posted messages. Windows has no second, more
compatible route today, so `ClickOptions::window_strategy`'s two variants
(`ChromiumCompatible`/`PidTargeted`) both resolve to this same delivery on Windows.

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
call draws all layers onto an off-screen 32-bpp DIB section with GDI, converts a
sentinel BGRA value to full transparency, and presents the composited frame via
`UpdateLayeredWindow`.

Readiness assessment (`assess_readiness`) treats `interactive_session = Missing` as
the only hard blocker. Elevation and UIAccess are diagnostic only — a
non-elevated process without UIAccess is the normal automation posture.

App-level activation resolves the process's most relevant window via the
`main_visible` selector (foreground → titled → largest), foregrounds it, then
verifies by re-observing the foreground window's owning process name.
