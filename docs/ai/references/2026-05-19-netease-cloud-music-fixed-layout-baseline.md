# NetEase Cloud Music Fixed-Layout Baseline

Date: 2026-05-19

Status: superseded baseline, not promoted

This fixed-layout baseline has been superseded by the window-scoped OCR design
and should not be treated as validated after the 2026-05-20 migration.

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

- `app probe` no longer uses the English metadata name `NeteaseMusic` as the
  OCR sample query when the live surface exposes the localized foreground name
  `网易云音乐`
- that change upgrades the sample OCR pass from a false-zero to weak visible
  title-level anchors
- `app analyze` can now emit one `window-primary-region` annotation from the
  AX root window fallback
- `app analyze` can also carry title-level `ocr-visible-text` anchors such as
  `网易云音乐` and `© 网易云音乐`
- that annotation now carries `window_bounds`, `relative_x`, and `relative_y`
  bindings for one conservative window-relative target
- `app distill` can now emit one
  `window-action.window-point.pointer-click.capture-evidence` candidate
- `app validate` can now auto-ground those bindings and validate one
  activation-level window-relative pointer slice live

That is useful progress, but it still does not produce:

- semantic search-entry grounding
- semantic result-selection grounding
- validated playback truth through the V2 path

The current rerun also makes the next bottleneck clearer:

- this is **not** primarily blocked by the verification provider for the current
  window-action slice; that slice already validates through runtime execution
  plus captured evidence
- the bigger gap is still candidate insufficiency for list-like or result-like
  targets
- the current OCR sample can now see title-level text such as `网易云音乐`, but
  that is still not an honest result-selection candidate

So the next product question is not "can we verify more?" first.
It is "can analyze emit a real list/result candidate shape for this app at all?"

## Current Honest Classification

The right classification for this NetEaseMusic slice is:

- `local-validated-recipe`
- `fixed-layout baseline`
- `phase-2 input`
- `window-relative pointer slice validated`
- `activation-level only`
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
