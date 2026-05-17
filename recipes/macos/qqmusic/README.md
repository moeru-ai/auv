# QQ音乐 macOS Recipes

This directory holds executable recipe manifests for the first QQ音乐 macOS
validation slices.

Current baseline:

- `open-search-submit-query.v0.json`
- `select-result-anchor.v0.json`
- `search-ocr-anchor.v0.json`
- `play-visible-anchor.v0.json`
- `play-visible-anchor.cases.v0.json`

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
current validation is:

1. run the broader `search-ocr-anchor` chain
2. double-click a visible row anchor
3. press the result play control
4. verify the now-playing title through the AX tree

That is enough to validate one narrow playback baseline, but it is not yet a
generic `qqmusic.play_song` contract.

There is now also a row-based fallback experimental wrapper:

- `scripts/local/qqmusic-play-visible-row.sh`

And a corresponding experimental row recipe:

- `recipes/macos/qqmusic/play-visible-row.v0.json`

This variant is meant to validate visible-row activation when Chinese OCR
anchors are not reliable enough for grounding.

The current row fallback uses a two-step activation chain:

1. click the visible row
2. press the result play control

That is the first version that is actually supposed to reach playback, not
just row selection.

The canonical local wrapper now goes through the product-facing Rust entrypoint:

- `cargo run --quiet -- skill run macos.qqmusic.play_visible_anchor.v0`

It no longer treats "all subcommands returned completed" as good enough. The
formal playback recipe now carries step-level expectations so the skill fails
when OCR resolves zero matches or when the expected now-playing title is absent
from the captured evidence image.

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
- the current stable playback proof is AX-based now-playing title verification,
  not screenshot OCR over the captured post-click image
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

cargo run --quiet -- skill run \
  macos.qqmusic.play_visible_anchor.v0 \
  --dry-run
```

## Why This Exists

The point is to stop carrying the QQ音乐 baseline as a chat transcript or an
ad-hoc shell sequence. A recipe manifest gives later agents a stable,
inspectable chain they can replay, override, and eventually distill further.

The product-facing CLI entrypoint is now:

```bash
auv-cli skill list
auv-cli skill show macos.qqmusic.play_visible_anchor.v0
auv-cli skill run macos.qqmusic.play_visible_anchor.v0 --dry-run
```

The machine-readable case matrix for this narrow skill currently lives at:

- `recipes/macos/qqmusic/play-visible-anchor.cases.v0.json`

The machine-readable case matrix for the row-based fallback currently lives at:

- `recipes/macos/qqmusic/play-visible-row.cases.v0.json`

Current product-facing coverage commands are:

```bash
auv-cli skill cases list
auv-cli skill cases show macos.qqmusic.play_visible_anchor.v0
auv-cli skill cases run macos.qqmusic.play_visible_anchor.v0 --dry-run
auv-cli skill cases run macos.qqmusic.play_visible_anchor.v0
auv-cli skill cases run macos.qqmusic.play_visible_anchor.v0 --case chinese-query-chinese-anchor --all-statuses
```

Current case-matrix truth:

- `ascii-aa-cure-for-me` is validated
- `ascii-aa-soft-universe` is validated
- `ascii-aa-aa-alone-again` is validated
- `chinese-query-chinese-anchor` is still a candidate and currently fails at
  `resolve-ocr-anchor`

Current row-fallback case truth:

- `ascii-aa-row-fallback` is validated
- `chinese-query-row-fallback` is validated for row-based fallback activation
- Chinese validated case now records `requested_title=晴天` and verifies `target_title=天空仍灿烂`
- Row-fallback verification now prefers AX tree title matching over screenshot OCR for the current now-playing title
- Chinese target-title disambiguation is not yet proven through row fallback; the current validated case proves activation on the Chinese result page, not semantic song selection

Coverage reporting entrypoints are now:

```bash
auv-cli skill cases report macos.qqmusic.play_visible_anchor.v0
auv-cli skill cases report macos.qqmusic.play_visible_row.v0
```

The current verification direction is:

1. use row detection to activate a visible result row
2. press the result play control
3. verify the now-playing title through the AX tree
