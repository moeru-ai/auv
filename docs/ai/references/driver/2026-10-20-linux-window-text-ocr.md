# Linux window text OCR (`window.findText`, `window.waitForText`)

Date: 2026-10-20

## Problem

`window.findText` and `window.waitForText` refused on Linux with `unsupported`
("only available on macOS and Windows"), although the Linux driver already had
the complete path: `WindowApi::find_text` / `wait_text` capture the resolved
window and run the shared Tesseract decoder, and `window.capture` is declared
supported on Linux.

The refusal was not a missing capability. The invoke command handlers were
compiled only for macOS and Windows, and the support cells said `Unsupported`
for Linux.

## Change

- `find_window_text` and `wait_for_window_text` are compiled on Linux and use
  `recognize_window_text_with_session`, the same helper the other platforms
  use. Linux resolves the window through AT-SPI, captures the display, crops
  the AT-SPI frame, and runs the shared OCR decoder.
- `recognize_window_text` (library helper) follows the same gate.
- The support cells for both commands are `linux: Supported`; the public
  evidence claim in `docs/SUPPORT_MATRIX.md` stays `not claimed` for Linux OCR.
- The Linux-only test that pinned the refusal now pins the dry run instead.

## Path

```text
resolve window (AT-SPI)
  -> list displays and capture (portal screenshot fallback)
  -> crop to the AT-SPI window frame
  -> tesseract
  -> matches in logical screen coordinates
```

`window.waitForText` polls the same path until a match appears or the timeout
expires; a timeout returns `not_found`.

## Evidence

- `cargo test -p auv-cli-invoke --lib window_text_commands_dry_run_on_linux`.
- `screen.findText "Workspace"` found the desktop's own text at 0.9
  confidence in this session, so portal capture and the OCR decoder work here.
- Live, GNOME Shell 50.4 Wayland on 2026-10-20: with a text file open in
  `gnome-text-editor`, `window.findText "hello auv ocr probe 42"` returned one
  match at `(2045.2, 127.4)` with confidence 0.89, inside the window frame
  `(2032, 55, 700×1077)`. `window.waitForText "hello auv"` returned at once,
  and `"no such text here"` with `--timeout-ms 800` failed after 800 ms with
  the timeout message.
- The off-screen case is refused, not guessed: the other AT-SPI window (Ghostty)
  sat at `x = 4672` on a PaperWM strip and `window.findText` reported the
  frame/bounds mismatch instead of OCRing the wrong pixels. Visible windows that
  do not expose AT-SPI do not appear in `window.list` at all.

## Boundaries

- The capture is a display screenshot cropped to the AT-SPI frame, so it sees
  only the visible part of the window. Occluded content and content outside the
  display are missing; macOS captures the window surface, Linux does not.
- A window manager that offsets window actors makes the frame/bounds check
  fail instead of returning text from another window. PaperWM keeps off-screen
  windows on a horizontal strip and the AT-SPI frame carries the strip
  position, so the check fails. Clamping the crop to the display was rejected:
  it would OCR whatever is visible at that coordinate.
- OCR quality depends on the tesseract data installed in the target
  environment.
- `window.clickText` stays macOS/Windows; it needs input delivery on top of the
  same OCR path and is a separate slice.

## Reproduction

```sh
cargo test -p auv-cli-invoke --lib window_text_commands_dry_run_on_linux
auv invoke screen.findText "Workspace" --json        # capture + OCR in this session
auv invoke window.findText "Workspace" --title "<visible window title>" --json
```
