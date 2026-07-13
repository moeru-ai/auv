# Notes AX Text Sample

Date: 2026-05-17

Status: validated native-app sample

## Why this exists

QQ音乐 proved the narrow skill shape across two playback verification
contracts: `verifyImageText` for the OCR-anchor slice and
`verifyNowPlayingTitle` for the row-fallback slice.
Notes proves the same runtime can carry that pattern into a second native macOS
app without screenshot OCR.

See also:

- `docs/ai/references/archive/skill-bundle/2026-05-17-native-app-skill-tree.md`

## Validated chain

Live replay showed this chain works:

1. `debug.activateApp`
2. `debug.pressButton` for `新建备忘录`
3. `debug.focusTextInput` for `Note Body Text View`
4. `debug.pasteTextPreserveClipboard` with `AUV_NOTE_MARKER_2026_05_16`
5. `debug.verifyAxText` on the body `AXTextArea`

## What it proves

- AX tree text verification can replace screenshot OCR for native text areas.
- Clipboard-backed paste is the stable body-entry path for this Notes sample.
- The reusable skill shape is not QQ音乐-specific.

## What it does not prove

- It does not prove generalized multi-app skill distillation.
- It does not prove browser or music-player reuse.
- It does not prove that every native app exposes a usable AX text area.

## Evidence

- Recipe: `recipes/macos/notes/create-and-verify-note.v0.json`
- Case matrix: `recipes/macos/notes/create-and-verify-note.cases.v0.json`
- Successful live replay: `run_1778947574511_68037_4`
