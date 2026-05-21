# Notes macOS Recipes

This directory holds the first native-app cross-surface sample for AUV.

Current baseline:

- `create-and-verify-note.v0.json`
- `create-and-verify-note.cases.v0.json`
- `../../docs/ai/references/2026-05-17-auv-native-app-skill-tree.md`

Phase 2 contract-consuming variant (candidate, not yet validated on live
desktop):

- `create-and-verify-note.v1.json`
- `create-and-verify-note.cases.v1.json`
- `../../docs/ai/references/2026-05-21-phase-3-first-contract-consumer-design.md`

v1 swaps the create-note step from `debug.pressButton` to
`debug.axPressButton` and asserts the Phase 2 contract via
`expect.signal_equals` (`cursorDisturbance=none`,
`pressMechanism=ax-action`, `performedAction=AXPress`). The recipe-level
disturbance budget remains `pointer` because `focus-body` still uses
`debug.focusTextInput`; adding a `debug.axFocusTextInput` primitive is
the next Phase 3 driver-side work item before the chain can become fully
keyboard-only.

What it proves:

1. activate Notes
2. create a new note
3. focus the note body through AX
4. write a stable marker through clipboard-backed text entry
5. verify the marker through the AX tree with `debug.verifyAxText`

This baseline deliberately avoids screenshot OCR. It is meant to show that
AUV can distill a reusable native-app skill shape from the QQ音乐 work into a
different macOS app with a real `AXTextArea`.

Replay:

```bash
cargo run --quiet -- skill run macos.notes.create_and_verify_note.v0

# v1 is candidate-only; run cases with --all-statuses to include it.
cargo run --quiet -- skill cases run \
  macos.notes.create_and_verify_note.v1 --dry-run --all-statuses
cargo run --quiet -- skill cases run \
  macos.notes.create_and_verify_note.v1 --all-statuses
```

Validated case (v0):

- `notes-marker-baseline`

Candidate case (v1):

- `notes-marker-ax-press`

The current marker is:

- v0: `AUV_NOTE_MARKER_2026_05_16`
- v1: `AUV_NOTE_MARKER_2026_05_21_V1`
