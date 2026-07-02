# AUV Scan S1-4a: Multi-frame Artifacts — Implementation Handoff

**Date:** 2026-07-03  
**Status:** landed — two-frame fixture producer + replay  
**Prerequisite:** [S1 slice 2 handoff](2026-07-02-auv-scan-s1-slice2-producer-handoff.md), [S1 slice 3 handoff](2026-07-02-auv-scan-s1-slice3-read-side-handoff.md)

## Boundary

**Owning crate:** `crates/auv-scan` only. **No** motion wire, `scroll_scan`, `run_read`, or viewer in this slice.

## S1-2 write-path alignment

| Constraint | Implementation |
| --- | --- |
| Single write path | `produce_frames_from_fixture_dir` loops `write_frame_with_image` per frame |
| Fail-closed | Per-frame rollback from slice 2 + batch rollback if a later frame write fails |
| Sequence identity | Fixture rejects duplicate `frame_id` and duplicate `sequence_index` before any write |

## Stable public API (new in S1-4a)

| Symbol | Role |
| --- | --- |
| `ProducedFrameBatch` | `{ produced: Vec<ProducedFrame> }` |
| `produce_frames_from_fixture_dir` | Hermetic multi-frame producer |
| `replay_scan_frames_from_dir` | Read-only replay (= `load_scan_frames_from_dir`) |

## Fixture: `two_frame_v0`

`crates/auv-scan/tests/fixtures/scan/temporal/two_frame_v0/` — manifest with `frames[]`, 2 PNG, golden JSON ×2.

## Tests added (7)

`produce_two_frame_fixture_*`, `two_frame_ids_are_unique`, `load_scan_frames_from_dir_returns_two_sorted`, `replay_scan_frames_does_not_invoke_capture`, `produce_two_frame_fixture_rejects_duplicate_sequence_index`, `produce_two_frame_fixture_rolls_back_on_late_write_failure`

## Validation

`cargo test -p auv-scan`
