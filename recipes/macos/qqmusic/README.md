# QQ音乐 macOS Recipes

This directory holds executable recipe manifests for the first QQ音乐 macOS
validation slices.

Current baseline:

- `open-search-submit-query.v0.json`
- `select-result-anchor.v0.json`
- `search-ocr-anchor.v0.json`
- `play-visible-anchor.v0.json`

The lower-disturbance baseline proves only the following chain:

1. open the QQ音乐 search surface through a keyboard shortcut
2. paste and submit a query while restoring the clipboard
3. capture post-submit evidence

It avoids pointer primitives, but it still foregrounds QQ音乐 and temporarily
uses the clipboard.

Current input truth:

- ASCII query submission is validated
- Chinese query submission is also validated through `pasteTextPreserveClipboard`
- Chinese OCR anchor resolution is **not** validated yet
- therefore Chinese search-entry is proven, but Chinese result-selection
  recipes should not yet assume OCR can resolve Chinese anchors

The broader result-selection baseline proves the following chain:

1. open the QQ音乐 search surface through a keyboard shortcut
2. paste and submit a query while restoring the clipboard
3. dismiss the lingering search suggestion overlay
4. resolve a known OCR anchor inside the result-list region
5. click the OCR anchor
6. capture post-click evidence

The modular result-selection skill proves only:

1. reactivate QQ音乐 so OCR does not accidentally inspect a different frontmost app
2. resolve a result-list OCR anchor inside the constrained visual region
3. click that anchor
4. capture post-click evidence

It does **not** prove playback activation yet.

There is now a narrower experimental playback wrapper:

- `scripts/local/qqmusic-play-visible-anchor.sh`

And now also a formal playback recipe:

- `recipes/macos/qqmusic/play-visible-anchor.v0.json`

It is intentionally not advertised as a generic recipe manifest yet. The
current validation still depends on one captured-image verification trick:

1. run the broader `search-ocr-anchor` chain
2. double-click a visible row anchor
3. capture post-click evidence
4. run OCR against the captured evidence image, restricted to the bottom-player
   title region

That is enough to validate one narrow playback baseline, but it is not yet a
generic `qqmusic.play_song` contract.

The recipe runner now applies a per-app live-desktop lock, so QQ音乐 recipes
for the same app instance do not execute concurrently through
`scripts/recipes/run_recipe.py`.

Current disturbance truth:

- the validated result-selection recipe still has `max_disturbance=pointer`
- this is no longer because search entry needs the pointer
- it is because stable result selection still depends on OCR/pointer fallback

The narrower search-entry recipe has `max_disturbance=clipboard` because it
avoids pointer primitives, but still foregrounds QQ音乐 and temporarily uses
the clipboard.

The broader result-selection recipe now also carries visual anchor constraints:

- OCR matching can be limited to a normalized screenshot region
- QQ音乐 defaults now target the result-list band instead of scanning the whole screen
- later 网易云 recipes can reuse the same region-constrained anchor approach

Current playback-validation truth:

- the first validated activation path is a row double-click, not a semantic AX
  action
- the first validated playback proof is OCR over the captured post-click image,
  not live-screen OCR
- the current baseline is specific to a known visible anchor and a known player
  title string
- this is strong enough for a narrow playback baseline, but still too narrow to
  advertise as a general-purpose QQ音乐 playback skill

Also be honest about concurrency:

- clipboard-backed primitives are now serialized with a global clipboard lock
- that does **not** make QQ音乐 itself concurrency-safe
- do not run multiple QQ音乐 recipes against the same live app instance at once

Probe evidence suggests QQ音乐 may admit a keyboard-first search-entry path,
but that is not yet the current recipe contract.

## How to Replay

Dry-run without touching the desktop:

```bash
python3 scripts/recipes/run_recipe.py \
  recipes/macos/qqmusic/open-search-submit-query.v0.json \
  --dry-run

python3 scripts/recipes/run_recipe.py \
  recipes/macos/qqmusic/search-ocr-anchor.v0.json \
  --dry-run
```

Replay with the convenience wrapper:

```bash
DRY_RUN=1 ./scripts/local/qqmusic-search-entry.sh aa
./scripts/local/qqmusic-search-entry.sh aa
./scripts/local/qqmusic-search-entry.sh 周杰伦

./scripts/local/qqmusic-search-entry-sentinel.sh

DRY_RUN=1 ./scripts/local/qqmusic-select-result.sh aa "Cure For Me"
./scripts/local/qqmusic-select-result.sh aa "Cure For Me"

DRY_RUN=1 ./scripts/local/qqmusic-select-visible-anchor.sh "Cure For Me"
./scripts/local/qqmusic-select-visible-anchor.sh "Cure For Me"

DRY_RUN=1 ./scripts/local/qqmusic-play-visible-anchor.sh aa "Cure For Me" "Cure For Me - AURORA"
./scripts/local/qqmusic-play-visible-anchor.sh aa "Cure For Me" "Cure For Me - AURORA"

python3 scripts/recipes/run_recipe.py \
  recipes/macos/qqmusic/play-visible-anchor.v0.json \
  --dry-run
```

## Why This Exists

The point is to stop carrying the QQ音乐 baseline as a chat transcript or an
ad-hoc shell sequence. A recipe manifest gives later agents a stable,
inspectable chain they can replay, override, and eventually distill further.
