# Windows window.clickText (Direct Route)

Status: Implemented and empirically verified on branch `feat/windows-clicktext-w1`.
This is slice W1 of Windows Parity ([#211](https://github.com/moeru-ai/auv/issues/211)).

## Scope and Boundaries

This slice wires the direct invoke route for `window.clickText` on Windows in
`crates/auv-cli-invoke/src/commands/window.rs`, closing the TODO marker
`invoke-window-click-text-windows`.

In scope:
- Direct invoke route `window.clickText` on Windows (`auv invoke window.clickText <TEXT>`).
- Routing through existing driver capabilities under `InputPolicy::BackgroundOnly`
  (`session.window().capture` + `session.vision().find_text_in_capture` +
  `session.window().to_window_point` + `session.window().click`).
- Preserving typed failures (`FailureCode::NotFound` for missing text,
  `FailureCode::InvalidInput` for out-of-range candidate index).
- Fixing the test surface: lengthening the test receiver window's button caption to
  `"Activate Target"` and widening the control for reliable OCR recognition.
- Empirical verification of semantic effect, coordinate mapping (PR #323), and
  unmodified background invariants across 5 live trials.
- Updating the Windows support cell from `Unsupported` to
  `Partial("test-owned Win32 surface, background-only")`.

Out of scope:
- `screen.clickText` (foreground slice S1).
- Runner-route support cells (#358).
- `app.activate`, `focusText`, or overlay presentation.
- Any latency or performance optimization claims.

## Implementation Architecture

On Windows, `window.clickText` reuses the established driver primitives without creating
a parallel click path:

```text
auv invoke window.clickText "Activate Target" --target win32-receiver.exe
  -> auv::local::open() (WindowsDriverSession)
  -> session.window().resolve(selector)
  -> session.window().capture(&window) (PrintWindow / WGC)
  -> session.vision().find_text_in_capture(&capture, &query) (Windows.Media.Ocr)
  -> matched candidate selection (defaults to index 0)
  -> point = ScreenPoint::from(matched.action_point())
  -> window_point = session.window().to_window_point(&window, point)
  -> session.window().click(&window, window_point, options)
       where options.policy == InputPolicy::BackgroundOnly
       -> post_window_click (WM_LBUTTONDOWN / WM_LBUTTONUP via PostMessageW)
```

### Invariants Maintained
- **Background-Only**: Input is delivered via posted window messages (`WM_LBUTTONDOWN`,
  `WM_LBUTTONUP`) directly to the target HWND. The desktop mouse cursor is not moved,
  and no global synthetic input (`SendInput`) is injected.
- **Zero Disturbance**: Monitored by `DesktopWatch` (low-level hooks `WH_MOUSE_LL`,
  `WH_KEYBOARD_LL`, and `EVENT_SYSTEM_FOREGROUND`). During all 5 trials:
  - `injected_mouse_total = 0`
  - `injected_keys_total = 0`
  - `foreground_taken = 0` (the target window never stole foreground focus).

## Empirical Evidence Run

Live trials were executed on 2026-10-14 against a test-owned native Win32 receiver
(`win32-receiver.exe`) covered by an occluding cover window (`cover_fraction: 1.0`).
Records are durable in `docs/ai/references/driver/2026-10-14-windows-window-clicktext.jsonl`
(run ID `1791978932-12276`).

| # | Trial Type | Query | Clicks | Frame Hash Differs? | Screen Point | Window Point | Latency | Verdict |
|---|------------|-------|--------|---------------------|--------------|--------------|---------|---------|
| 1 | Positive | "Activate Target" | 0 $\to$ 1 | Yes (`9844...` $\to$ `af10...`) | (2254.0, 1113.5) | (107.0, 63.5) | 213.2 ms | Pass (BN_CLICKED processed) |
| 2 | Positive | "Activate Target" | 1 $\to$ 2 | Yes (`af10...` $\to$ `ad34...`) | (2254.0, 1113.5) | (107.0, 63.5) | 308.1 ms | Pass (BN_CLICKED processed) |
| 3 | Positive | "Activate Target" | 2 $\to$ 3 | Yes (`ad34...` $\to$ `c4a3...`) | (2254.0, 1113.5) | (107.0, 63.5) | 227.9 ms | Pass (BN_CLICKED processed) |
| 4 | Coordinate (PR #323) | "Activate Target" | 3 $\to$ 4 | Yes (`c4a3...` $\to$ `f722...`) | (434.0, 303.5) | (107.0, 63.5) | 232.9 ms | Pass (Offset window hit) |
| 5 | Negative | "NonExistentTargetText_404" | 4 $\to$ 4 | No (`f722...` $\equiv$ `f722...`) | N/A | N/A | 203.7 ms | Pass (Typed NotFound, 0 clicks) |

### Semantic Verification
Unlike delivery-only assertions, each positive trial proved semantic consumption:
- The receiving Win32 control received `WM_LBUTTONDOWN` / `WM_LBUTTONUP` and triggered
  `BN_CLICKED`, incrementing its verified click counter (`state.clicks`).
- The receiver's UI updated its status label (`"clicks N"`) and painted state strip,
  producing different capture hashes before and after delivery.

### PR #323 Regression Verification (Trial 4)
In Trial 4, the target window was moved to screen offset `(320, 240)` with frame
`[327.0, 240.0, 366.0, 293.0]` and button screen rect `[344, 287, 524, 319]`.
- Resolved screen point: `(434.0, 303.5)`, landing squarely inside the button rect.
- Resolved window point: `(107.0, 63.5)`.
- Verified that `screen_point != window_point`, confirming that screen-space OCR coordinates
  are correctly projected into window-relative coordinates without dropping to `(0, 0)` or
  double-counting window decorations.

### Negative Trial (Trial 5)
- Missing query `"NonExistentTargetText_404"` cleanly exited with code 1 and typed error:
  `window.clickText did not find text "NonExistentTargetText_404"`.
- Receiver click count remained unchanged at 4 (`clicks_delivered: 0`).
- Pre- and post-invocation window capture hashes were bit-for-bit identical (`f722f040eb09e0c6`).

## Support Declaration Update

In `crates/auv-cli-invoke/src/commands/window.rs`, `window.clickText` support is updated:

```rust
#[invoke_command(
  id = "window.clickText",
  support = {
    macos: Supported,
    linux: Unsupported,
    windows: Partial("test-owned Win32 surface, background-only")
  },
  ...
)]
```
