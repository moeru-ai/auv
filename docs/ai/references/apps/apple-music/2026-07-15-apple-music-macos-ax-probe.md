# Apple Music macOS AX surface probe

**Status:** bounded discovery only (no click, no play, no candidate algorithm)

## Scope

This slice adds `auv-apple-music probe-macos`, a bounded AX discovery probe for
Music.app on macOS:

1. Activate `com.apple.Music` via `ApplicationControl::activate_bundle_id`
   (no `CGWindowID` / WindowServer discovery required, same seam as PR #106).
2. Capture the AX tree via `AccessibilityApi::capture_app_tree`.
3. Filter observed nodes into two candidate lists:
   - `search_field_candidates`: nodes matching `AXTextField`/`AXSearchField`
     role/subrole, or a search-related placeholder/title.
   - `result_row_candidates`: `AXRow`/`AXStaticText` nodes with non-empty
     title or value.
4. Optionally persist the raw AX snapshot as JSON to `--artifact-dir`.

## Explicitly not done in this slice

- No click/press on any discovered node.
- No playback action (play, pause, transport).
- No `OperationResult` / `StepOutcome` contract (this is a probe command, not
  a product operation).
- No support-matrix row or evidence-level claim.
- No general candidate-selection algorithm. The `is_search_field_candidate`
  and `is_result_row_candidate` filters are heuristic role/text matchers for
  probe output only; they are not the deterministic candidate contract that
  `app.apple_music.search_and_play` will need.

TODO(apple-music-search-and-play): candidate selection, action delivery, and
now-playing verification are deferred until this probe's real AX shape from
Music.app is captured on real macOS hardware. Do not implement
`search_and_play` from this doc alone — it describes the shape AUV has not
yet observed. Unlocked once an owner reviews a live-captured `--artifact-dir`
snapshot and approves the next slice (search query submission, deterministic
result selection, or now-playing verification) as a narrow follow-up.

## Manual command

```sh
cargo run -p auv-apple-music --bin auv-apple-music -- \
  probe-macos --artifact-dir /tmp/auv-music-probe --json
```

## Live result

Not yet run on real macOS hardware. This doc records the bounded scope of the
probe; a follow-up evidence note should record actual `search_field_candidates`
and `result_row_candidates` output once run live.
