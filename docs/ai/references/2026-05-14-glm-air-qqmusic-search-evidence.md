# GLM Air QQ Music Search Evidence

Date: 2026-05-14

Status: validated local evidence note

## Purpose

This note records two things:

- the first verified QQ Music macOS search recipe that AUV can currently repeat
- evidence that a lightweight GLM Air model can restate that recipe into a
  structured artifact for later distillation work

This is not proof of a full playback skill. It is proof of a repeatable
`search -> OCR anchor resolve -> OCR anchor click -> evidence capture` slice.

## Proven Recipe Slice

Validated command chain:

1. `debug.focusTextInput`
2. `debug.typeText`
3. `debug.findScreenText`
4. `debug.clickScreenText`
5. `debug.captureScreen`

Baseline target and inputs:

- target app id: `com.tencent.QQMusicMac`
- reveal shortcut: `cmd+f`
- reveal settle: `300ms`
- submit settle: `900ms`
- baseline query: `aa`
- baseline OCR anchor: `I DRINK THE LIGHT`

The corresponding normalized recipe artifact lives in:

- `docs/ai/references/2026-05-14-qqmusic-search-ocr-anchor-skill-v0.json`

## Local Validation Evidence

The recipe was rerun locally more than once and produced the same core outcome:
QQ Music search results were reached, the OCR anchor was resolved, the OCR
anchor click completed, and a post-click screenshot was captured.

Representative run ids:

- focus: `run_1778749173096_56263_0`
- type and submit: `run_1778749175475_56494_0`
- OCR resolve: `run_1778749178853_57073_0`
- OCR click: `run_1778749180210_57371_0`
- post-click screenshot: `run_1778749181988_57588_0`

Key observed facts from those runs:

- `debug.findScreenText --query "I DRINK THE LIGHT"` returned at least one OCR
  match in the screenshot.
- `debug.clickScreenText --query "I DRINK THE LIGHT"` projected the OCR anchor
  to logical point `498.750,655.750` and completed the click.
- The post-click screenshot showed the `I DRINK THE LIGHT (Jengi Remix)` row
  selected/highlighted.

Still not proven:

- row activation
- playback start
- playback verification

## GLM Air Distillation Call

The recipe was sent to `glm-4-air-250414` as a minimal structured-distillation
prompt.

Call metadata:

- model: `glm-4-air-250414`
- request id: `20260514171311e0d48da2670e448f`
- prompt tokens: `178`
- completion tokens: `277`
- total tokens: `455`

Prompt intent:

> Distill a minimal reusable recipe for a macOS QQ Music black-box search flow,
> using only proven facts, and return JSON only.

The model returned a compact JSON recipe containing:

- skill id
- target app
- required commands
- ordered steps
- verification section
- known limits section

Important review note:

The model output was not accepted blindly. It was normalized against the live
AUV validation results before being committed as the skill JSON. In particular:

- the committed skill keeps the explicit reveal step and settle timings
- the committed skill keeps `submit_key=return` instead of pretending a raw
  newline is the contract
- the committed skill preserves the limitation that playback activation is still
  unverified

## Why This Matters

This is enough to justify the next phase:

- a lightweight model can consume a narrow, verified recipe surface
- the recipe can be carried by explicit commands instead of free-form screen
  improvisation
- future distillation does not need to start from raw `screenshot -> guess ->
  click` loops for this exact slice

It is not enough to claim:

- that QQ Music playback is already a stable skill
- that the full app workflow is distilled
- that low-end models can yet complete the whole task without stronger
  verification and activation logic
