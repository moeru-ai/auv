# Linux live desktop matrix evidence

Date: 2026-10-20

Evidence for the Linux column of `docs/SUPPORT_MATRIX.md`: what was run, on
which desktop, and what the results mean. Input delivery is covered by the
separate [EIS live evidence](2026-10-19-linux-portal-eis-input.md#live-evidence)
from 2026-10-19 and was not re-run here.

## Environment

- GNOME Shell 50.4 Wayland with PaperWM, one display: 2752×1152 logical,
  3440×1440 physical, scale 1.25.
- Portal RemoteDesktop v2 through `xdg-desktop-portal-gnome`. The AUV portal
  app id (`ai.moeru.auv`) is not installed, so PipeWire screencast registration
  fails and capture falls back to the Screenshot portal.
- Tesseract data installed in the development shell.

## Results

| command | result |
| --- | --- |
| `window.list` | Completed with the AT-SPI windows (GNOME Shell "Main stage" and the app windows). A Gtk4 window reported `(0, 0)` and received the shell-stage origin. 683 a11y calls and 0.10–0.14 s after the stage-walk fix; 17,289 calls and 2.4–2.9 s before it ([latency note](2026-10-20-linux-window-list-latency.md)). |
| `display.capture` | Completed through `xdg-desktop-portal.screenshot.crop`: bounds 2752×1152, pixels 3440×1440, scale 1.25. `fallback_reason` names the PipeWire registration failure. |
| `window.capture` | Completed on a visible Gtk4 text editor window: frame `(20, 55, 700×1077)`, pixels 875×1346. The first attempt was refused with "changed frame during display capture" while PaperWM animated the new window; three retries completed. |
| `window.findText` | Completed; `"hello auv matrix probe"` at `(33.2, 123.8)`, confidence 0.86, inside the window frame. |
| `window.waitForText` | Completed on the same window; absent text failed after 800 ms with a readable timeout message. |
| `screen.findText` | Completed; `"Workspace"` at `(12, 11.2)`, confidence 0.92. |

## Boundaries

- One display, one GNOME session, one window manager (PaperWM). An off-screen
  PaperWM column keeps its strip coordinate in the AT-SPI frame, so
  `window.capture` and `window.findText` refuse it with a frame/bounds error
  instead of reading the wrong pixels.
- `window.capture` is a display crop: occluded pixels are returned as-is, and a
  frame that changes between the before/after snapshots is refused.
- Windows that do not expose AT-SPI do not appear in `window.list`; the browser
  and VS Code on this desktop were not listed.
- OCR quality depends on the installed tesseract data; the probes are one line
  of text, so multi-line and non-Latin coverage is not claimed.
- These runs do not change any command support cell by themselves; they are the
  evidence the matrix cells cite.

## Reproduction

```sh
cd crates/auv && cargo test -p auv-driver-linux --lib

# window.list
target/debug/auv invoke window.list --json

# display and window capture
target/debug/auv invoke display.capture --json
target/debug/auv invoke window.capture --target app:org.gnome.TextEditor --json

# text anchors (open a text file in the editor first)
target/debug/auv invoke window.findText "hello auv matrix probe" --target app:org.gnome.TextEditor --json
target/debug/auv invoke window.waitForText "hello auv" --target app:org.gnome.TextEditor --json
target/debug/auv invoke screen.findText "Workspace" --json
```
