# Windows screen and foreground commands through invoke

Status: live evidence for 13 Windows commands that had a direct local path in source and no run
behind it ([coverage audit](2026-10-11-invoke-platform-coverage-audit.md#result)). This is test
and docs work for [#211](https://github.com/moeru-ai/auv/issues/211). It adds eval scenarios and
receiver features, and changes no product code. The owner asked for the live runs on 2026-10-12,
while the desktop was free.

## Scope and boundaries

Run the real `auv` binary for:

- `display.list`, `display.capture`, `screen.captureRegion`
- `screen.findText`, `screen.waitForText`
- `input.moveMouse`, `input.clickPoint`, `input.drag`
- `input.typeText`, `input.key`, `input.keys`, `input.holdKeys`, `input.keyboard`

Each result is judged against what a window the harness owns saw. The driver's report about itself
is not the measure.

Not in this slice:

- Code changes to these commands. The runs found no defect in them.
- `input.pasteText` and the `paste_text` action of `input.keyboard`. On Windows both are refused,
  and the runs record the refusal: exit 1, the text unchanged, no key sent. The driver has no
  `paste_text` capability (`TODO(windows-paste-batch)`). Update: the
  [Windows paste note](../driver/2026-10-12-windows-paste-text.md) adds the capability, and the
  `foreground` scenario now asserts the paste instead of the refusal.
- The selected Runner path. No Runner claim is made.

## How the runs stay on the harness's window

These commands move the real pointer and send real keys; that is what they do. The two scenarios
keep them on the harness's own receiver:

- **The receiver is created topmost.** It is a detached process that holds no foreground right.
  A window that opens without activating goes under the foreground window, which is often
  maximized. `SetWindowPos(HWND_TOPMOST)` returned success and was ignored for such a window,
  whether it came from the receiver or from the harness (`topmost=false` afterwards). Creating the
  window with `WS_EX_TOPMOST` works (`receiver.rs --topmost`).
- **Points are checked.** Before a command aims the real pointer at a point, the harness checks
  that the point belongs to the receiver (`WindowFromPoint`, then `GetAncestor(GA_ROOT)`).
- **The user is idle.** Before every command that moves the pointer or sends keys, the harness
  waits until the mouse and keyboard have been idle for 2 s. If that does not happen within 30 s,
  it gives up.
- **Keys only reach the receiver.** Keys are sent only while the receiver is the foreground window
  and its text box has the focus. Otherwise the remaining keyboard commands are skipped and the run
  fails.
- **The desktop is put back.** At the end the pointer returns to where it was, and the foreground
  goes back to the window that had it, however the run ends.
- **The screenshot is not kept.** `display.capture` takes a screenshot of the user's screen. It
  goes to an invoke store that is removed afterwards. Only the receiver's decoded state strip is
  written down. The records hold window handles and no window titles or process names.

The desktop witness (`watch.rs`) runs around every command. It records injected and physical input,
the pointer and foreground changes.

- In the **foreground** scenario, five commands must leave the desktop alone and are judged on
  that: `display.list`, the window-relative `input.clickPoint` and `input.drag`, `display.capture`
  and `screen.captureRegion`.
- The commands that act are judged on what the receiver saw. They fail the run only if a window of
  another process becomes the foreground.

## Live evidence

Host: Windows 11 Pro 10.0.29648, one display at 2560x1440 and 100% scaling, UAC off. This was an
interactive console session that the user was also working in. Text recognition used the system
OCR. The text box has no input method by default (`ImmAssociateContext(.., HIMC::default())`); see
[the IME finding](#key-presses-and-an-active-ime).

```sh
cargo build -p auv-cli
just eval windows screen --output <fresh dir> --auv target/debug/auv.exe
just eval windows foreground --output <fresh dir> --auv target/debug/auv.exe
just eval windows foreground --keep-ime --output <fresh dir> --auv target/debug/auv.exe
```

Evidence: [`evidence/2026-10-12-windows-screen-and-foreground-commands/`](evidence/2026-10-12-windows-screen-and-foreground-commands/).

| Run | Result | Witness |
| --- | --- | --- |
| `foreground-1.jsonl` | PASS | 5 quiet commands: 3 held, 2 inconclusive (user mouse movement), 0 violated |
| `foreground-2.jsonl` | PASS | 5 held |
| `foreground-3.jsonl` | PASS | 5 held |
| `foreground-keep-ime.jsonl` | PASS, key text recorded and not judged | 5 held |
| `screen-1-no-settle.jsonl` | PASS | 7 held |
| `screen-2-no-settle.jsonl` | PASS | 2 held, 5 inconclusive |
| `screen-3-no-settle-missed-label.jsonl` | FAIL, the first `findText` found nothing | 7 inconclusive |
| `screen-4.jsonl` | PASS | 1 held, 6 inconclusive |
| `screen-5.jsonl` | PASS | 1 held, 6 inconclusive |
| `screen-6-foreign-media-keys.jsonl` | FAIL, witness only | 1 held, 4 inconclusive, 2 violated |

Runs 1 to 3 of `screen` ran before the scenario waited 600 ms after showing the receiver; see
[the fade-in](#text-read-while-the-window-fades-in). An inconclusive verdict means the user moved
the mouse during the command, so a pointer change cannot be attributed. Apart from the media keys
in `screen-6`, no command that must leave the desktop alone saw any injected input.

What the passing runs showed:

- **`display.list`** reported one display, `[0, 0, 2560, 1440]` at scale 1, in 41 to 49 ms.
- **`display.capture`** returned 2560x1440 pixels under bounds `[0, 0, 2560, 1440]`, from the
  `xcap.windows` backend.
- **`screen.captureRegion`** of the receiver's window rectangle returned pixels whose decoded
  state strip equals the receiver's own state, the same as for `display.capture`. A strip only
  decodes when the bounds place the image on the screen correctly. Latency was 610 to 627 ms for
  the display capture and 67 to 71 ms for the region.
- **`screen.findText "clicks 0"`** found the label's text 1 px right of and 3 px below the label's
  corner, and `"line 000"` found the edit's text inside the edit. After the receiver moved by
  (-500, -300), the text was found moved by exactly (-500, -300). One full-screen recognition took
  0.95 to 1.6 s.
- **`screen.waitForText`**:
  - text already present: returned after one pass (0.95 to 1.6 s);
  - text that never appears: gave up after the default 5 s with "did not find text" (5.1 to 5.9 s);
  - text that appeared 1.5 s after it started: found it (3.0 to 4.3 s).
- **`input.clickPoint`, relative to the window** (`--target window:<hwnd> --relative-to window`),
  took the `window_targeted_mouse` path:
  - the receiver's button counted one more click;
  - no input was injected;
  - the foreground did not change.
- **`input.drag`, relative to the window**, put the pad's button-down and button-up exactly at the
  requested points, with 19 moves and 301 ms between them. It also injected no input.
- **`input.moveMouse`** left the real pointer exactly at the requested point.
- **`input.clickPoint` on the display** (`--target display:<id>`) took the
  `foreground_system_events` path. The button counted the click, and the receiver became the
  foreground window, as it would for a user's click.
- **`input.drag` on the screen**:
  - the pad recorded button-down and button-up exactly at the requested points;
  - 18 moves, with the button held 300 to 301 ms;
  - the pointer was left at the end point.
- **`input.clickPoint` on the screen** at the text box gave it the focus.
- **The keyboard commands built `auv typedaBCdddEfgh`** in the text box, one step at a time, with
  the text checked after each step:
  - `input.typeText "auv typed"`;
  - `input.key a`, `input.key shift+b`;
  - `input.keys shift c`, `input.keys d --count 3 --interval-ms 40`;
  - `input.holdKeys shift e --duration-ms 400`: the text box saw shift held for 402 to 403 ms;
  - `input.keyboard` with a `press f` action and a `type_text "gh"` action.
- **`input.pasteText ij` and the `paste_text` action of `input.keyboard`** both exited 1. Neither
  changed the text, and the text box received no key. (These records predate the paste capability;
  see the [Windows paste note](../driver/2026-10-12-windows-paste-text.md) for the paste runs.)
- **The run restored the desktop**: the pointer went back to where it started and the foreground
  went back to the window that had it.
- Each foreground run injected 29 mouse and 44 key events, all from the commands that act.

## Findings

### Key presses and an active IME

On this machine the user's input method is Microsoft Pinyin. While it is active for the focused
control:

- The key presses of `input.key`, `input.keys`, `input.holdKeys` and the `press` action of
  `input.keyboard` reach the application as `VK_PROCESSKEY` (`0xE5`) on key-down. Key-up arrives
  with the real virtual key, and no text appears at once.
- `input.typeText` and the `type_text` action send `KEYEVENTF_UNICODE` input (`VK_PACKET`, `0xE7`),
  which the IME does not compose. Its text appears at once.

The presses are not lost. They stay in the IME composition, and Windows commits that composition
when the window loses the foreground. In `foreground-keep-ime.jsonl` the final text is
`auv typedghaBCdddEf`: the typed `gh` got in ahead of the composed `aBCdddEf`.

So on a desktop with an active IME, the key commands can deliver every key and still not produce
their text when the command returns. The text comes later, in another order, or not at all if the
composition is cancelled. AUV does not detect this, and the result looks the same as a plain
success.

The scenario turns the IME off for its own text box by default and has `--keep-ime` to reproduce
the finding. AUV itself does not switch input methods.

### Text read while the window fades in

In `screen-3-no-settle-missed-label.jsonl`, the first `screen.findText "clicks 0"` found nothing.
It ran 1.3 s after the receiver was shown. The next command, a second later, read the edit's text.
Windows animates a window that has just been shown, and an OCR pass during the fade can miss
low-contrast text. That is the likely cause, but this record does not prove it.

The scenario now waits 600 ms after showing the receiver, with a `NOTICE:` at the wait. With the
wait, all three later runs found the label.

The same race can meet any caller that reads the screen right after a window appears. AUV's text
commands do not wait for animations. `screen.waitForText` absorbs the race; `screen.findText` does
not.

### Input another program injected

In `screen-6-foreign-media-keys.jsonl`, the witness saw two injected media-key presses during the
two `screen.waitForText` commands. They came with extra-info tag `0x0` while the user was moving
the mouse.

`screen.waitForText` captures and reads the screen. It sends no input. AUV's own `SendInput` also
uses tag `0`, so the witness cannot tell the two apart by tag.

Other programs on this machine inject input. This is the limit recorded in
[injected key presses during bench](../driver/2026-10-10-windows-background-session.md#injected-key-presses-during-bench),
and it is judged `violated` as before.

## Not evidenced

- Display scaling other than 100%, more than one display, and virtual desktops, remote sessions
  and locked sessions. `screen.findText` and `screen.waitForText` cover the primary display only.
- Applications other than the receiver. Whether an application acts on a window-relative
  `input.clickPoint` or `input.drag` (posted messages) depends on its toolkit; see the
  [background session by application](../../../SUPPORT_MATRIX.md#windows-background-session-by-application).
- Right and middle buttons, double clicks, and click modifiers. Drags with other buttons.
- Keys other than letters with shift, and other keyboard layouts.
- The key commands with an active IME, beyond the finding above.
- OCR beyond the receiver's two lines of text.
- Windows of a higher integrity level (UIPI). This machine runs every process at high integrity
  with UAC off.
- The selected Runner path.

## Validation

```sh
cargo fmt --check
cargo clippy -p auv-base-evals --all-targets
cargo test --workspace --locked --no-fail-fast
git diff --check
just eval windows screen --output ... --auv target/debug/auv.exe
just eval windows foreground --output ... --auv target/debug/auv.exe
just eval windows foreground --keep-ime --output ... --auv target/debug/auv.exe
```

## Candidate next slices

- ~~`input.pasteText` on Windows: a `paste_text` capability in the driver, then the command.~~
  Done in the [Windows paste note](../driver/2026-10-12-windows-paste-text.md).
- Report an active input method in the result of a key command, so callers can tell
  "keys delivered" apart from "text typed". This is a typed metadata gap that the IME finding
  exposed.
- The same scenarios at 125% or 150% scaling and on a second display.
