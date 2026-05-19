# NetEase Cloud Music Fixed-Layout Baseline

Date: 2026-05-19

Status: local validated baseline, not promoted

## Purpose

This note records the current NetEaseMusic macOS baseline that landed in
`recipes/macos/netease-cloud-music/`.

The important boundary is simple:

- this is a real narrow recipe
- it is locally validated for one fixed layout
- it is **not** yet promoted into the frozen phase-1 native-app bundle

That distinction matters because V2 is supposed to promote validated slices
through an explicit workflow, not by pretending every working local recipe is
already part of the product truth set.

## What Exists

The repo now carries:

- `recipes/macos/netease-cloud-music/play-visible-anchor.v0.json`
- `recipes/macos/netease-cloud-music/play-visible-anchor.cases.v0.json`

The current case is:

- `aurora-cure-for-me-fixed-layout`

It proves a narrow chain:

1. activate and capture the NetEaseMusic window
2. click a fixed search-box point
3. paste and submit `AURORA Cure For Me`
4. verify the visible result page through OCR on the captured window
5. double-click a fixed first-result point
6. verify `Cure For Me` and `AURORA` in the bottom-player image region

## Why It Is Not Promoted Yet

The current recipe still depends on fixed global logical coordinates:

- `search_click_x=3509`
- `search_click_y=398`
- `result_click_x=3457`
- `result_click_y=727`

It also depends on a validated local double-click interval:

- `click_interval_ms=80`

That means the current baseline is:

- real
- useful
- inspectable

but still only a fixed-layout local slice.

It is not yet a promoted bundle member because the current V2 workflow only
re-expresses part of this slice so far:

- `app analyze` can now emit one `window-primary-region` annotation from the
  AX root window fallback
- `app distill` can now emit one
  `window-action.window-point.pointer-click.capture-evidence` candidate
- `app validate` keeps that candidate in `candidate` because `relative_x` and
  `relative_y` are still unresolved

That is useful progress, but it still does not produce:

- semantic search-entry grounding
- semantic result-selection grounding
- validated playback truth through the V2 path

## Current Honest Classification

The right classification for this NetEaseMusic slice is:

- `local-validated-recipe`
- `fixed-layout baseline`
- `phase-2 input`
- `window-relative pointer candidate available`
- `not yet promoted`

The wrong classification would be:

- generalized NetEaseMusic playback skill
- frozen phase-1 native-app member
- reusable semantic song-selection contract

## What This Baseline Is Good For

It is still valuable because it gives V2 a second music-player sample with a
different failure shape than QQMusic:

- bundle-id resolution can be flaky
- the UI is not yet represented through stable annotation objects
- the working chain currently uses fixed points
- the validated double-click timing matters

That makes it a good stress sample for:

- selector coherence
- candidate / annotation layer design
- window-relative pointer candidate distillation
- activation-vs-semantic verification boundaries

## Next Product Step

Do not force this recipe directly into the frozen skill tree.

The next step should be:

1. keep the recipe as a truthful local baseline
2. keep using it to pressure-test V2 candidate / annotation contracts
3. ground the fixed points into honest window-relative targets
4. only promote it after the workflow can describe and validate it without lying

## Related Files

- `recipes/macos/netease-cloud-music/README.md`
- `recipes/macos/netease-cloud-music/play-visible-anchor.v0.json`
- `recipes/macos/netease-cloud-music/play-visible-anchor.cases.v0.json`
- `docs/ai/references/2026-05-19-v2-docs-contract.md`
