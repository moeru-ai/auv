# auv-driver-windows

Windows desktop driver for AUV. Exposes the same capability-oriented session
surface as `auv-driver-macos` and `auv-driver-linux`, adapted for Win32,
UIA, and `Windows.Media.Ocr`.

## Capabilities

- Display list / capture / region capture
- Window list, resolve, capture (`GDI PrintWindow`)
- Window mutation: move, resize, set-frame, minimize, restore, zoom
- Window-targeted click/scroll (foreground-only; no UIA background pointer path — see `TODO(windows-window-targeted-background-input)`)
- Global click, scroll, type text, press key, copy, paste (`SendInput`)
- Clipboard snapshot / restore / set-text
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
| `TODO(windows-window-targeted-background-input)` | `src/session.rs` | UIA/background input path; only foreground `SendInput` exists today |
| `TODO(windows-input-target-lease)` | `src/input.rs` | Typed input lease matching the macOS slice |
| `TODO(windows-window-mutation-fallback)` | `src/mutation.rs` | Foreground/SendInput fallback for mutation when the Win32 API is blocked |
| `TODO(windows-window-zoom-verification)` | `src/mutation.rs` | Zoom/maximize state verification after mutation |
| `TODO(windows-window-capture-dpi)` | `src/capture.rs` | Tighter DPI/border mapping for window capture |
| `TODO(windows-ax-value-write)` | `src/accessibility.rs` | UIA `ValuePattern` writes |
| `TODO(windows-clipboard-rich-formats)` | `src/clipboard.rs` | Rich/non-text clipboard snapshot and restore |
| `TODO(windows-driver)` | `src/descriptor.rs` | Extend capability strings as slices land |
| `TODO(app-activate-windows-cli)` | `crates/auv-cli-invoke/src/commands/app.rs` | Wire `activate_process_name` into the `app.activate` CLI command (needs an owner decision on the shared output contract) |
| `TODO(driver-overlay-windows-motion)` | `crates/auv-driver-overlay-windows/src/overlay.rs` | Per-layer position easing / animation |
| SVG cursor rasterization (see `NOTICE` in `src/window.rs`) | `crates/auv-driver-overlay-windows/src/window.rs` | `CursorImage::Svg` is rejected with `Err`; needs a rasterizer |

## Architecture notes

Input delivery uses `SendInput` exclusively. Every `WindowInput` path foregrounds the
target window first (`SetForegroundWindow`/`ShowWindow`), so all click/scroll results
carry a fallback reason. This mirrors how `auv-driver-linux` handles the
RemoteDesktop portal.

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
