# QQ Music Play Visible Anchor Evidence Pack

Date: 2026-05-15

This folder contains the curated evidence pack for the first validated QQ音乐
macOS playback baseline:

`search -> OCR anchor resolve -> row double-click -> evidence capture -> captured-image OCR verification`

It is intentionally selective. It does not mirror the whole `.auv/` runtime
directory.

## Included Files

- `glm-air-request.json`
  - Sanitized request payload sent to `glm-4-air-250414`
- `glm-air-response.json`
  - Raw model response used as the starting point for the normalized playback
    skill JSON

Additional curated artifacts may be added here as this playback baseline
expands beyond one narrow row-activation slice.

## What This Pack Proves

- AUV can replay one narrow QQ音乐 playback baseline.
- A lightweight GLM Air model can restate that validated playback slice into a
  structured JSON skill artifact.

## What This Pack Does Not Prove

- generalized playback for arbitrary rows
- pointer-free playback activation
- Chinese OCR anchor stability for result selection

The normalized playback skill artifact is stored separately at:

- `docs/ai/references/2026-05-15-qqmusic-play-visible-anchor-skill-v0.json`
