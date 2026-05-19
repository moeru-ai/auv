# NetEaseMusic macOS Recipes

This directory holds narrow NetEaseMusic automation recipes discovered through
`cargo run --quiet -- invoke ...` exploration.

Current baseline:

- `play-visible-anchor.v0.json`
- `play-visible-anchor.cases.v0.json`

The current recipe is being revalidated after migrating from fixed global
coordinates to window-scoped OCR:

1. activate and capture the NetEaseMusic window
2. click the search box by a window-relative point
3. paste and submit `AURORA Cure For Me`
4. capture the search result page
5. wait for `Cure For Me` inside the resolved window
6. double-click the window-scoped OCR anchor for `Cure For Me`
7. capture the post-play window
8. verify `Cure For Me` and `AURORA` in the bottom-player image region

It intentionally does not claim generalized NetEaseMusic playback. The recipe
uses window-relative search-box focus and window-scoped OCR result activation.
It should survive window movement across displays as long as the target window
can be resolved by `debug.listWindows` and remains single-display contained.

`click_interval_ms=80` is part of the validated contract. Earlier immediate
`click_count=2` events were too fast for stable NetEaseMusic result activation.

The migrated recipe is marked `needs-revalidation` until a live run proves the
new window-scoped flow.

Dry-run:

```bash
cargo run --quiet -- skill run macos.netease_cloud_music.play_visible_anchor.v0 --dry-run
```

Live run:

```bash
cargo run --quiet -- skill run macos.netease_cloud_music.play_visible_anchor.v0
```

Case run:

```bash
cargo run --quiet -- skill cases run macos.netease_cloud_music.play_visible_anchor.v0 \
  --case aurora-cure-for-me-fixed-layout
```
