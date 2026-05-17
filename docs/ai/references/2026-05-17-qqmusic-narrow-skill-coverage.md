# QQ音乐 Narrow Skill Coverage

Date: 2026-05-17

Status: active productization coverage note

## Purpose

This note consolidates the current QQ音乐 narrow-skill coverage into one place.

The point is to stop pretending the repo already has "QQ音乐 support" as a
single solved capability. It does not. What exists now is a productized narrow
slice with two separate playback strategies and explicitly declared failure
boundaries.

## Executable Coverage Entry Points

```bash
auv-cli skill cases report macos.qqmusic.play_visible_anchor.v0
auv-cli skill cases report macos.qqmusic.play_visible_row.v0

auv-cli skill cases run macos.qqmusic.play_visible_anchor.v0
auv-cli skill cases run macos.qqmusic.play_visible_row.v0
```

## Strategy Split

### 1. OCR Anchor Playback

Recipe:

- `recipes/macos/qqmusic/play-visible-anchor.v0.json`

Coverage matrix:

- `recipes/macos/qqmusic/play-visible-anchor.cases.v0.json`

What it validates:

- ASCII query submission
- constrained result-region OCR anchor resolution
- double-click anchor activation
- captured-image verification of now-playing title

What is actually validated:

- `ascii-aa-cure-for-me`
- `ascii-aa-soft-universe`
- `ascii-aa-aa-alone-again`

What is not validated:

- Chinese OCR anchor playback

Current Chinese boundary:

- `chinese-query-chinese-anchor`
- query: `周杰伦`
- requested/anchor title: `晴天`
- current outcome: `resolve-ocr-anchor` fails with zero OCR matches

This means Chinese OCR anchor playback is still a candidate boundary, not a
supported product path.

### 2. Row Fallback Playback

Recipe:

- `recipes/macos/qqmusic/play-visible-row.v0.json`

Coverage matrix:

- `recipes/macos/qqmusic/play-visible-row.cases.v0.json`

What it validates:

- result-page row detection
- visible-row activation
- explicit play-control press
- AX-based now-playing verification

What is actually validated:

- `ascii-aa-row-fallback`
- `chinese-query-row-fallback`

The important nuance:

- `ascii-aa-row-fallback` proves a stable row-based playback fallback for the
  current ASCII baseline.
- `chinese-query-row-fallback` proves that the Chinese result page can go
  through the row-fallback activation path and land on a verifiable
  now-playing title.

It does **not** prove semantic requested-title selection.

The Chinese row case currently records:

- `requested_title = 晴天`
- `verified target_title = 天空仍灿烂`

That is not a minor wording issue. It means the current row fallback is a
validated activation strategy on the Chinese result page, but not yet a
validated "play the requested song" strategy for Chinese result selection.

## Current Product Truth

The honest product-level statement is:

- QQ音乐 narrow playback is productized enough to expose formal recipes, case
  matrices, coverage reports, disturbance budgets, and verification contracts.
- ASCII playback is validated through both OCR-anchor and row-fallback paths.
- Chinese playback is split:
  - Chinese search entry is validated.
  - Chinese OCR anchor playback is not validated.
  - Chinese row fallback activation is validated.
  - Chinese semantic requested-title selection is not validated.

## What To Claim

Acceptable claims:

- "QQ音乐 has a productized narrow playback slice on macOS."
- "ASCII QQ音乐 playback is validated through two strategies."
- "Chinese QQ音乐 result-page activation has a validated row fallback."
- "Chinese OCR anchor playback is still a candidate boundary."

Do not claim:

- "QQ音乐 playback is broadly solved."
- "Chinese QQ音乐 playback is fully supported."
- "Row fallback semantically selects the requested Chinese song."
- "The current narrow skill generalizes to arbitrary queries or layouts."

## Why This Matters

This repo is now past the "demo script" phase. That is good.

But productization without truthful coverage language is just better-packaged
self-deception. The narrow skill is useful precisely because its supported
surface and its failure boundaries are both explicit.
