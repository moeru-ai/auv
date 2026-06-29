# QQ Music Playback Verification Baseline

Date: 2026-05-15

Status: narrow validated baseline

## Purpose

This note records the first QQ音乐 playback-validation slice that is strong
enough to reuse, while still being honest about how narrow it is.

The point is not to claim a general `qqmusic.play_song` skill. The point is to
capture one reproducible activation-and-verification chain that later agents
can inspect, rerun, and eventually distill further.

## Validated Chain

The current validated chain is:

1. open the QQ音乐 search UI
2. submit the query `aa`
3. dismiss the search overlay
4. resolve the visible result-row anchor `Cure For Me`
5. double-click the resolved row anchor
6. capture post-click evidence
7. run OCR over the captured evidence image
8. verify the player-title region contains `Cure For Me - AURORA`

The first practical wrapper for this baseline was a retired `scripts/local/`
helper:

```bash
./scripts/local/qqmusic-play-visible-anchor.sh aa "Cure For Me" "Cure For Me - AURORA"
```

The formal recipe and normalized skill artifact now live at:

- `recipes/macos/qqmusic/play-visible-anchor.v0.json`
- `docs/ai/references/2026-05-15-qqmusic-play-visible-anchor-skill-v0.json`

## Why This Is Narrow

This baseline still depends on:

- a visible row anchor that OCR can resolve
- pointer disturbance for result activation
- a known player-title string for post-click verification
- OCR over a captured evidence image instead of a semantic playback API

It now also depends on the recipe runner exporting the capture step's image
artifact path into the later verification step. That makes the playback chain
machine-replayable without keeping shell-specific parsing logic as the source of
truth.

That means the baseline is good enough to prove a real playback slice, but not
good enough to advertise a general-purpose QQ音乐 playback skill.

## Verification Contract

The current verification step is intentionally conservative:

- do not recapture the live desktop again
- verify the already captured post-click evidence image
- restrict OCR to the bottom-player title region

Current region defaults:

- `left = 0.22`
- `top = 0.80`
- `right = 0.45`
- `bottom = 0.90`
- `min_confidence = 0.90`

Current verified title query:

- `Cure For Me - AURORA`

## What This Proves

- AUV can reach a QQ音乐 search results page.
- AUV can resolve a visible result-row anchor through constrained OCR.
- AUV can activate one known row through a double-click.
- AUV can verify the resulting now-playing state from a captured evidence image.

## What This Does Not Prove

- playback activation for arbitrary search results
- pointer-free playback activation
- generalized row activation semantics
- Chinese OCR anchor stability in result selection
- a reusable end-user-facing `qqmusic.play_song` contract

## Why Captured-Image OCR Matters

Live-screen OCR in the player-title region was less stable than OCR over the
captured post-click evidence image. The captured-image path is therefore the
current truth source for playback verification.

This is not as elegant as a semantic player-state API, but it is more honest
for the current QQ音乐 black-box surface.
