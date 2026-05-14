# QQ Music Search OCR Anchor Evidence Pack

Date: 2026-05-14

This folder contains the curated evidence pack for the first validated QQ Music
macOS recipe slice:

`search -> OCR anchor resolve -> OCR anchor click -> evidence capture`

It is intentionally selective. It does not mirror the whole `.auv/` runtime
directory.

## Included Files

- `qqmusic-search-aa.png`
  - Screenshot after submitting the baseline query `aa`
- `qqmusic-search-aa-contract.txt`
  - Screenshot coordinate contract for the search-state capture
- `ocr-find-I-DRINK-THE-LIGHT.txt`
  - OCR detection report showing the anchor text match
- `ocr-click-I-DRINK-THE-LIGHT.txt`
  - OCR click projection report with the resolved logical point
- `qqmusic-result-anchor-aa.png`
  - Post-click screenshot evidence showing the selected result row
- `qqmusic-result-anchor-aa-contract.txt`
  - Screenshot coordinate contract for the post-click capture
- `glm-air-request.json`
  - Sanitized request payload sent to `glm-4-air-250414`
- `glm-air-response.json`
  - Raw model response used as the starting point for the normalized skill JSON

## What This Pack Proves

- AUV can reach QQ Music search results with the current baseline recipe.
- AUV can resolve a visible OCR anchor from a desktop screenshot.
- AUV can project that OCR anchor back into logical click coordinates.
- AUV can click the anchor and capture post-click evidence.
- A lightweight GLM Air model can restate the validated slice into a structured
  JSON recipe.

## What This Pack Does Not Prove

- playback activation
- playback verification
- a finished QQ Music playback skill

The normalized recipe artifact is stored separately at:

- `docs/ai/references/2026-05-14-qqmusic-search-ocr-anchor-skill-v0.json`
