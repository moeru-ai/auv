# Windows window commands through invoke

Status: implemented and merged in #336. This is a feature slice of
[#211](https://github.com/moeru-ai/auv/issues/211): the Windows rows of the `window.*` group. The
slice was named in the session that built it.

## Scope and boundaries

Connect four direct local commands to the existing typed Windows driver:
`window.list`, `window.capture`, `window.findText` and `window.waitForText`.
They reuse the results, reports and tracing artifacts that macOS and Linux already use.
Selected Runner execution stays a separate fact: this slice makes no Runner claim.

Not in this slice, each with a marker at the place where the deferral lives:

- `window.clickText`. The Windows `WindowApi` has no window-targeted click, and
  `input().click_at` is foreground input. The only window-targeted click is `BackgroundSession`,
  a Rust API that `invoke` does not reach (`TODO(invoke-window-click-text-windows)`).
- `screen.findText`, `screen.waitForText`, `screen.clickText`, `input.*`, `app.*`, `overlay.*`
  and `mediaControl.*` on Windows. Overlay presentation reports `unavailable` on Windows, as on
  Linux, and does not block observation.
- Naming an application by anything other than its executable file name
  (`TODO(cross-platform-application-selector)` keeps the existing deferral).
- The typed support information of #211. It is a separate, cross-platform slice.

## Application target on Windows

An `app:` target, and a bare `--target`, is the **executable file name** shown as APP in
`window.list`, for example `notepad.exe`. The driver matches it exactly, ignoring ASCII case:
the same program is `Notepad.exe` on disk and `notepad.exe` to the person typing it. The `.exe`
suffix may be left out (`notepad`). Titles stay case-sensitive, as on macOS.

Windows reports no application id yet (`Window.application_id` is always `None`). The local invoke
path selects an application by name and a Runner client sends an application id
(`WindowSelector.application_id`);
since #366 the Windows driver matches both against the executable name, so the two paths agree.
`--title` works as on the other platforms. A selector that names nothing is an error, not another
window; when no visible window belongs to the requested application, the error lists the visible
applications (`application "x" (visible applications: …) was not found`).

The shared target help says `app:<application-id>`. The Windows meaning is stated in the command help
of the four commands, as the Linux slice did, instead of rewriting the shared text.

## Defects the live check found

The first live run of the new commands against a window the harness owns found two defects in the
existing Windows driver. Both reproduce on the unchanged driver (the "before" runs below) and are
fixed here.

| Defect | Before | After |
| --- | --- | --- |
| `PrintWindow` renders the whole `GetWindowRect` rectangle, which includes the invisible resize border (7 px left, right and bottom on this machine). `Capture.bounds` was the DWM frame, which leaves it out. | 380x300 pixels under 366x293 bounds, scale 1.038. The receiver's click strip decoded as `(0, 0)`, not `(3, 0)`. Recognized text sat up to 6 px from where it is drawn. | 366x293 pixels under the same bounds, scale 1. The strip decodes as `(3, 0)`. |
| `window.capture` of a minimized window answered with its 146x28 stub at (-32000, -32000) as a successful capture. | exit 0, bounds `[-31993, -32000, 146, 28]`. | exit 1, "window is minimized and cannot be captured; AUV does not restore windows". The window stays minimized. |

The driver now crops the bitmap to the visible frame, so the image and `Capture.bounds` describe
the same rectangle, and `bounds` is read when the pixels are captured, not copied from the window
snapshot. The window's recorded frame was already right; the pixels did not match it.

Unit tests next to the code:

- `visible_crop_*` (driver, `capture_test.rs`): the crop from the measured numbers, a frame equal
  to the rectangle, a window that moved between the two reads, and a frame that misses the bitmap.
- `app_name_matches_an_executable_name_ignoring_case` (driver, `window_test.rs`).
- `window_observation_commands_are_not_gated_out_on_windows`,
  `window_click_text_stays_unavailable_on_windows` and `window_selector_names_the_executable_on_windows`
  (invoke, `window_test.rs`). Restoring the old gate on `window.list` makes the first one fail
  with "window.list was refused on Windows". A dry run proves the gate and nothing else.

## Live evidence

Host: Windows 11 Pro 10.0.29648, one display at 2560x1440 and 100% scaling, UAC off, an
interactive console session with the user working in it. No elevated rights and no permission
grant were needed. Text recognition is the system OCR (`Windows.Media.Ocr`).

Command, which runs the real `auv` binary:

```sh
cargo build -p auv-cli
just eval windows window --output evals/auv-base/results/windows-window --auv target/debug/auv.exe
```

The scenario (`evals/auv-base/platforms/desktop-windows/tasks/window_commands.rs`) starts a
receiver the harness owns, moves it once so DWM reports its visible frame, covers it completely
with a second harness window, and posts three clicks to it. Then it runs `auv invoke` for each
command under the desktop witness and judges the answers against what the receiver itself knows:
the frame against DWM read by the harness, the saved capture by decoding the receiver's click
strip from its pixels, and recognized text against the screen rectangle of the control that draws
it. Every selector names the receiver's title, so no command resolves "the main window" and
captures what the user is working in. The invoke store is removed at the end because
`window.list` records every window on the desktop; the records keep only the receiver and a count.

Evidence: [`evidence/2026-10-11-windows-window-commands/`](evidence/2026-10-11-windows-window-commands/).

| Run | Build | Result | Witness |
| --- | --- | --- | --- |
| `window-before.jsonl` | the original `capture.rs` | FAIL, 3 checks | 2 held, 8 inconclusive, 0 violated |
| `window-after-1.jsonl` | this change | PASS | 5 held, 5 inconclusive, 0 violated |
| `window-after-2-foreground-moved.jsonl` | this change | FAIL, witness only | 6 held, 3 inconclusive, 1 violated |
| `window-after-3.jsonl` | this change | PASS | 10 held |

What the passing runs showed:

- `window.list` found the receiver by its handle with the executable name, process id, title and
  frame the receiver knows, among 17 listed windows, while a second receiver covered it.
- `window.capture --target win32-receiver.exe --title ...` returned the receiver's pixels, not
  the cover's, with bounds equal to its DWM frame and a click strip that decodes to its own state.
  [After](evidence/2026-10-11-windows-window-commands/window-capture-after.png) and
  [before](evidence/2026-10-11-windows-window-commands/window-capture-before.png) the fix.
- `window.findText "clicks 3"` and `"line 000"` found the text of the receiver's label and edit,
  inside their controls (`clicks 3` is 1 px right of and 3 px below its label's corner).
  `"Press"`, the caption of its button, is not found: the system OCR does not read a caption
  that short on a button face. It is recorded, not asserted.
- `window.waitForText` returned at once for text that is there (about 150 ms), gave up on text
  that never appears after the default five seconds with "did not find text", and caught text
  that appeared 1.5 s after it started (1.7 s).
- A selector naming an executable with no window failed. A minimized receiver was not restored.
- Latencies, whole `auv` process included: `window.list` 44 to 54 ms, `window.capture` 73 to
  78 ms and `window.findText` 140 to 300 ms once the binary had been launched before. The first
  run of a newly built binary shows 2.4 s for `window.list` and 0.5 s for `window.capture`; that
  is the first launch of the file, not the commands.

**The desktop witness did not pass every run.** In `window-after-2-foreground-moved.jsonl` the
foreground moved twice during the five-second wait for text that never appears: to the taskbar
(`Shell_TrayWnd`), then to a window of another program. The user's own activity was in progress:
162 physical mouse events in that command and two key presses in the commands before it. No
harness or target window took the foreground, no input was injected in any run (0 mouse, 0
keys), and the target's rectangle and minimized state did not change. The witness cannot attribute
a foreground change unless a physical click or key lies within two seconds of it, so the verdict
is `violated`. It is the same limit recorded in
[Foreground changes the witness cannot attribute](../driver/2026-10-10-windows-background-session.md#foreground-changes-the-witness-cannot-attribute).
The four commands call only the driver's `list`, `resolve` and `capture` (`EnumWindows`, DWM
attribute reads, `PrintWindow`) and the OCR engine. The driver's activation entry points
(`WindowApi::activate`, process-name activation and foreground input preparation) are not on that
path, and `capture` is called without activation options. That is a reading of the code; the
witness runs do not prove it from the desktop side.

## Not evidenced

- Display scaling other than 100%. The crop assumes the process sees physical pixels;
  `TODO(windows-window-capture-dpi)` stays at the capture code.
- More than one display, windows that span displays, virtual desktops, remote or locked sessions.
- Applications other than the harness receiver. `PrintWindow` with `PW_RENDERFULLCONTENT` is the
  capture backend, and whether it returns the content of GPU-drawn windows (browsers, XAML) is the
  question the `apps` survey asks of the background session, not yet of this path.
- OCR quality beyond the receiver's two lines of text, and OCR languages other than the
  profile's.
- `window.list` lists a minimized window with its off-screen frame and `is_visible: true`. Changing
  that alters how a background session discovers a window it must answer `skipped_minimized` for,
  so it is left as it was (`TODO(windows-window-list-minimized)` in `window.rs`).
- The selected Runner path.

## Validation

```sh
cargo fmt --check
cargo clippy -p auv-driver-windows -p auv-cli-invoke -p auv-base-evals --all-targets
cargo test -p auv-driver-windows -p auv-cli-invoke -p auv-base-evals
cargo check --target x86_64-unknown-linux-gnu -p auv-driver-windows --all-targets
git diff --check
just eval windows window --output ... --auv target/debug/auv.exe
```

Clippy reports no finding in a line this change adds. `auv-cli-invoke` cannot be checked for Linux
from a Windows host (`ring` needs a C cross compiler); its Windows edits are `cfg` extensions and
a Windows-only branch.

## Candidate next slices

- `screen.findText` and `screen.waitForText` on Windows: the OCR path exists; the question is
  that they read the user's screen.
- `input.pasteText` and the paste action of `input.keyboard`: the clipboard pieces exist, the
  driver has no `paste_text`.
- `overlay.*` on Windows: the adapter exists and the feature is already on.
- `app.probePermissions` and `app.activate` once the output contract is decided.
- The `apps` survey run against `window.capture`, to put per-toolkit evidence under the capture
  backend.
