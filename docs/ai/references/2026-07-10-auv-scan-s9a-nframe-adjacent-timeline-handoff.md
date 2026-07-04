# AUV Scan S9a: N-frame Adjacent Multi-Segment Timeline — Implementation Handoff

**Date:** 2026-07-10  
**Status:** implemented — `build_scan_timeline_from_bundle` emits N-1 adjacent segments when `len >= 2` (`landed proof` for adjacent multi-segment timeline builder; **not** track/identity continuity substrate)  
**Prerequisite:** [S1-4b two-frame timeline](2026-07-03-auv-scan-s1-s4b-motion-timeline-handoff.md), [S1 bounded contract review](2026-07-05-auv-s1-bounded-contract-graduation-review.md)

## Scope lock

**S9a only:** `scan-timeline-v0` builder contract revision — from「exactly 2 frames or `unsupported_frame_count`」to「`>= 2` frames => N-1 adjacent segments; `< 2` => `insufficient_frames`」. Wire schema unchanged.

**Related:** adjacent association tracks wire → [S9b](2026-07-10-auv-scan-s9b-adjacent-tracks-wire-handoff.md).

**NOT this slice:** scene_state semantic upgrade; `inspect_run` / `inspect_server`; `scan-tracks-v0`; association algorithm upgrade; continuity verdict / ID switch policy; runtime invoke / live capture.

## Builder policy (S9a)

| `frames.len()` | `segments` | `diagnostics` |
| --- | --- | --- |
| `< 2` | `[]` | `[insufficient_frames]` |
| `>= 2` | `N-1` adjacent pairs | `[]` (per-pair `motion_unknown` stays on segment) |

Motion per segment: `pub(crate) estimate_viewport_motion_between` in `motion.rs` (same `window_bounds` delta algorithm as S1-4b). Public `estimate_viewport_motion(bundle)` remains two-frame helper only (used by `scene_state.rs` unchanged).

## Legacy diagnostic

`DIAG_UNSUPPORTED_FRAME_COUNT` (`unsupported_frame_count`): **deprecated-by-production** — constant retained in `lib.rs` re-exports with `NOTICE(s9a-legacy)`; builder no longer emits.

## Fixtures / test hard gates

| Gate | Requirement |
| --- | --- |
| two-frame | `two_frame_v0` golden/manifest **unchanged** |
| three-frame | **required** `three_frame_v0` fixture + manifest-driven test (2 segments) |
| four-frame | handbuilt smoke only (`segments.len() == 3`); **no** golden fixture |

## Files touched

| File | Change |
| --- | --- |
| `crates/auv-scan/src/motion.rs` | `pub(crate) estimate_viewport_motion_between`; public two-frame helper unchanged |
| `crates/auv-scan/src/timeline.rs` | N-1 builder; `NOTICE(s9a-contract-revision)` |
| `crates/auv-scan/tests/fixtures/scan/temporal/three_frame_v0/` | New 3-frame fixture |

## Validation

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
git diff --check
```

## Graduation note

S9a = **landed proof** for N-frame **adjacent multi-segment timeline builder**. S2 **tracks substrate** row remains **`hold`** ([S9b](2026-07-10-auv-scan-s9b-adjacent-tracks-wire-handoff.md) landed `scan-tracks-v0` wire only). Motion semantics remain metadata-proxy **helper proof only**.
