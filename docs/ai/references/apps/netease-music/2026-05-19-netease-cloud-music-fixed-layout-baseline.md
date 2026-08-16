# NetEase Cloud Music Fixed-Layout Baseline

Date: 2026-05-19

Status: superseded baseline, not promoted

This fixed-layout baseline has been superseded by the window-scoped OCR design
and should not be treated as validated after the 2026-05-20 migration.

Historical reading boundary: the statements below describe the 2026-05-19
pre-retirement snapshot. The recipe files and the `app distill` / `app validate`
commands were later removed. The current `auv app` surface is `probe -> analyze`,
and reusable product operations now live in app-local Rust crates.

## Purpose

This note records the May 2026 NetEaseMusic macOS baseline that landed in
`recipes/macos/netease-cloud-music/`.

The recorded boundary was simple:

- this was a real narrow recipe
- it was locally validated for one fixed layout
- it was **not** promoted into the frozen phase-1 native-app bundle

That distinction mattered because V2 was supposed to promote validated slices
through an explicit workflow, not pretend every working local recipe was
already part of the product truth set.

## Recorded Baseline

The repo then carried:

- `recipes/macos/netease-cloud-music/play-visible-anchor.v0.json`
- `recipes/macos/netease-cloud-music/play-visible-anchor.cases.v0.json`

The recorded case was:

- `aurora-cure-for-me-fixed-layout`

It proved a narrow chain:

1. activate and capture the NetEaseMusic window
2. click a fixed search-box point
3. paste and submit `AURORA Cure For Me`
4. verify the visible result page through OCR on the captured window
5. double-click a fixed first-result point
6. verify `Cure For Me` and `AURORA` in the bottom-player image region

## Why It Was Not Promoted

The recorded recipe depended on fixed global logical coordinates:

- `search_click_x=3509`
- `search_click_y=398`
- `result_click_x=3457`
- `result_click_y=727`

It also depended on a validated local double-click interval:

- `click_interval_ms=80`

That meant the baseline was:

- real
- useful
- inspectable

but still only a fixed-layout local slice.

It was not a promoted bundle member because the V2 workflow only re-expressed
part of this slice:

- `app probe` used the localized foreground name instead of the English
  metadata name `NeteaseMusic` as the OCR sample query when the live surface
  exposed `网易云音乐`
- that change upgraded the sample OCR pass from a false-zero to weak visible
  title-level anchors
- `app analyze` could emit one `window-primary-region` annotation from the
  AX root window fallback
- `app analyze` could also carry title-level `ocr-visible-text` anchors such as
  `网易云音乐` and `© 网易云音乐`
- that annotation carried `window_bounds`, `relative_x`, and `relative_y`
  bindings for one conservative window-relative target
- `app distill` could emit one
  `window-action.window-point.pointer-click.capture-evidence` candidate
- `app validate` could auto-ground those bindings and validate one
  activation-level window-relative pointer slice live

That was useful progress, but it did not produce:

- semantic search-entry grounding
- semantic result-selection grounding
- validated playback truth through the V2 path

The recorded rerun also made the next bottleneck clearer:

- this was **not** primarily blocked by the verification provider for the
  recorded window-action slice; that slice already validated through runtime
  execution plus captured evidence
- the bigger gap was candidate insufficiency for list-like or result-like
  targets
- the recorded OCR sample could see title-level text such as `网易云音乐`, but
  that was not an honest result-selection candidate

The recorded product question was not "can we verify more?" first. It was "can
analyze emit a real list/result candidate shape for this app at all?"

## Historical Honest Classification

The recorded classification for this NetEaseMusic slice was:

- `local-validated-recipe`
- `fixed-layout baseline`
- `phase-2 input`
- `window-relative pointer slice validated`
- `activation-level only`
- `not yet promoted`

The unsupported classification would have been:

- generalized NetEaseMusic playback skill
- frozen phase-1 native-app member
- reusable semantic song-selection contract

## What This Baseline Is Good For

It remains useful as historical evidence because it gave V2 a second
music-player sample with a different failure shape than QQMusic:

- bundle-id resolution could be flaky
- the UI was not represented through stable annotation objects
- the working chain used fixed points
- the validated double-click timing mattered

That made it a good stress sample for:

- selector coherence
- candidate / annotation layer design
- window-relative pointer candidate distillation
- activation-vs-semantic verification boundaries

## Recorded Next Product Step

The note recorded this next step at the time:

1. keep the recipe as a truthful local baseline
2. keep using it to pressure-test V2 candidate / annotation contracts
3. ground the fixed points into honest window-relative targets
4. only promote it after the workflow can describe and validate it without lying

## Historical Files And Current References

These recipe paths were later removed and remain recoverable from Git history:

- `recipes/macos/netease-cloud-music/README.md`
- `recipes/macos/netease-cloud-music/play-visible-anchor.v0.json`
- `recipes/macos/netease-cloud-music/play-visible-anchor.cases.v0.json`

Current documentation:

- `docs/ai/references/ops/2026-05-18-app-probe-analyze-workflow.md`
- `docs/ai/references/ops/2026-05-28-surface-analyze-v0.md`
