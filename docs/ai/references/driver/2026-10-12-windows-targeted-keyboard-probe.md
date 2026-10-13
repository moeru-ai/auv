# Windows targeted keyboard: posted-message probe

Status: probe record for #349 ([#211](https://github.com/moeru-ai/auv/issues/211)). It replaces
`keyboard-targeted.md`, which #349 merged at the repository root (recoverable at `c265d461`). The
[consolidation review](../invoke-cli/2026-10-13-windows-parity-consolidation-review.md) has the
full reasoning.

This is a probe, not command evidence. The trials posted raw `WM_KEYDOWN`, `WM_KEYUP` and `WM_CHAR`
messages from a harness that was not committed, so they cannot be rerun. None of them went through
`auv invoke`, and targeted keyboard input has no route on Windows. The driver refuses it
(`TODO(windows-targeted-keyboard)` in `crates/auv-driver-windows/src/input.rs` and
`background_session.rs`).

## Cells: unchanged

`input.typeText`, `input.pasteText`, `input.key`, `input.keys`, `input.holdKeys` and
`input.keyboard` stay `Partial("foreground delivery only; targeted delivery is unsupported")` on
Windows:

- the foreground half is live-validated
  ([screen and foreground commands](../invoke-cli/2026-10-12-windows-screen-and-foreground-commands.md),
  [paste](2026-10-12-windows-paste-text.md));
- the targeted half is refused in source.

Two proposals from #349 were not taken:

- **`input.key`, `input.keys`, `input.holdKeys` → Unsupported.** That would erase a live-validated
  foreground path to describe the targeted path, which the note already excludes. #349's own final
  recommendation was to keep targeted keyboard unsupported.
- **A new `input.typeText` note.** It was half Chinese, and the note shows verbatim in `--help` and
  MCP. It also claimed a mechanism that the command cannot reach: `--target` resolves top-level
  windows, and the probe itself shows that a top-level `WM_CHAR` has no effect.

## What the probe suggests

Records: [`evidence/2026-10-12-windows-targeted-keyboard-probe/trials.jsonl`](evidence/2026-10-12-windows-targeted-keyboard-probe/trials.jsonl),
38 trials.

- **`WM_CHAR` posted to a classic `Edit` child appends text without the focus.** This held for a
  harness window of class `AUV_Notepad_Anchor_Thread_V2` (called "Notepad" in the records; it is
  not Notepad) and for the evals' `win32-receiver`.
- **The same text posted to the top-level window has no effect** (trials 37 and 38). Windows does
  not route a posted character from a frame window to its child control. A targeted route would
  have to resolve the child control.
- **A posted modifier does not change the key state the application reads.** `WM_KEYDOWN(VK_CONTROL)`
  leaves `GetKeyState(VK_CONTROL)` at 0 in the target thread, so Ctrl+A arrived as the letter `a`.
  This matches documented Win32 behavior. It is the same principle as for posted pointer input:
  [a posted message carries only what is in it](../../../TERMS_AND_CONCEPTS.md#background-session).
- **A targeted paste cannot be a posted Ctrl+V**, for the same reason. `WM_PASTE` to a classic
  `Edit` control would avoid the modifier, but it still needs a resolved child control and a
  recipient check before each action. It also needs the clipboard loan to attribute the read to
  the target process instead of the foreground process (`crates/auv-driver-windows/src/input.rs`,
  `paste_text`).

## What the records do not support

- **Trials 4 and 16** (backspace) record `semantic_effect: none`, yet each deleted two characters
  (trial 4: `abcd` to `ab`). The original document says backspace works.
- **Trials 11 and 23** (holding a letter) record seven repeated characters as `success`. Their
  `stop_reason` beside it says repeats do not happen in the background.
- **The Edge trials (25 to 36) say nothing about Chromium and keys.** They were posted to Edge's
  `Intermediate D3D Window`, a compositor surface, not its input window. They were judged by the
  window title, which typing into a page does not change. They also sent text, Return, Ctrl+A and
  Ctrl+C into the user's own Edge Beta. A Windows probe now acts only on windows it created
  (`evals/auv-base/platforms/desktop-windows/README.md`).
- **The IME claim is not in the records.** The original document says an active input method turns
  posted `WM_KEYDOWN` into `VK_PROCESSKEY`. The foreground runs did show an IME composing key
  presses (#340), but no targeted trial recorded it.

## What targeted keyboard input would need

Each item is a separate decision. None of them is approved:

- resolving a child control from `--target`, for example through UI Automation;
- a recipient lease that revalidates the recipient before each action (the existing TODO's
  trigger);
- a decision on modifiers, since posted messages cannot carry them;
- for paste, `WM_PASTE` on classic `Edit` controls, with the clipboard loan attributed to the
  target.

Until then, targeted keyboard input stays refused.
