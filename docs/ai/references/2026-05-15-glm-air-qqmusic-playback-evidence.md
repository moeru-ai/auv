# GLM Air QQ Music Playback Evidence

Date: 2026-05-15

Status: validated local evidence note

## Purpose

This note records two things:

- the first validated QQ音乐 macOS playback baseline that AUV can currently
  replay
- evidence that a lightweight GLM Air model can restate that baseline into a
  compact JSON skill artifact

This is not proof of a generalized QQ音乐 playback skill. It is proof of a
repeatable narrow slice:

`search -> OCR anchor resolve -> row double-click -> evidence capture -> captured-image OCR verification`

## Proven Playback Slice

Validated command chain:

1. `debug.pressKey`
2. `debug.pasteTextPreserveClipboard`
3. `debug.pressKey`
4. `debug.findScreenText`
5. `debug.clickScreenText`
6. `debug.captureScreen`
7. `debug.findImageText`

Baseline target and inputs:

- target app id: `com.tencent.QQMusicMac`
- reveal shortcut: `cmd+f`
- reveal settle: `300ms`
- submit settle: `900ms`
- validated query: `aa`
- validated row anchor: `Cure For Me`
- validated playback title: `Cure For Me - AURORA`
- validated activation mode: `double-click`

The corresponding normalized skill artifact lives in:

- `docs/ai/references/2026-05-15-qqmusic-play-visible-anchor-skill-v0.json`

The formal executable recipe lives in:

- `recipes/macos/qqmusic/play-visible-anchor.v0.json`

The curated raw evidence pack lives in:

- `docs/ai/references/evidence/2026-05-15-qqmusic-play-visible-anchor/`

## Local Validation Evidence

Representative successful recipe run:

- `open-search`: `run_1778820332176_12764_0`
- `paste-query`: `run_1778820333209_12906_0`
- `dismiss-search-overlay`: `run_1778820336257_12990_0`
- `resolve-ocr-anchor`: `run_1778820337011_13063_0`
- `double-click-row-anchor`: `run_1778820338751_13131_0`
- `capture-evidence`: `run_1778820341552_13276_0`
- `verify-player-title`: `run_1778820342330_13351_0`

Key observed facts from that run:

- the result-row anchor `Cure For Me` was resolved in the constrained results
  region
- the row double-click completed
- the captured evidence image existed at the end of the same recipe run
- `debug.findImageText` confirmed `Cure For Me - AURORA` in the bottom-player
  title region of that captured image

Still not proven:

- generalized playback for arbitrary rows
- pointer-free playback activation
- Chinese OCR anchor stability for result selection

## GLM Air Distillation Call

The playback baseline was sent to `glm-4-air-250414` as a structured
distillation prompt.

Call metadata:

- model: `glm-4-air-250414`
- request id: `20260515140242254a20de94e84a55`
- prompt tokens: `257`
- completion tokens: `334`
- total tokens: `591`

Prompt intent:

> Distill a narrow reusable playback skill for a macOS QQ Music black-box
> result activation flow, using only proven facts, and return JSON only.

The model returned a compact JSON skill containing:

- skill id
- target app
- required commands
- ordered high-level steps
- verification section
- known limits section

Important review note:

The model output was not accepted blindly. The committed skill artifact was
normalized against the live AUV recipe and runtime behavior before being kept in
the repo. In particular:

- the normalized artifact preserves the narrow baseline framing
- it does not claim generalized playback
- it does not claim pointer-free activation
- it keeps captured-image OCR verification as the current truth source

## Why This Matters

This is enough to justify the next phase:

- a lightweight model can consume a validated playback slice through an explicit
  recipe/skill surface
- future distillation does not need to start from raw screenshot improvisation
  for this exact slice
- the repo now contains both an executable recipe and a normalized skill
  artifact for the same baseline

It is not enough to claim:

- that QQ音乐 playback is solved in general
- that the full app workflow is distilled
- that the current baseline is robust across arbitrary queries, rows, or
  localized OCR anchors
