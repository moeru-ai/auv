# AUV Scan S1-4b: Two-frame Motion / Timeline — Implementation Handoff

**Date:** 2026-07-03  
**Status:** landed  
**Prerequisite:** [S1-4a multi-frame handoff](2026-07-02-auv-scan-s1-s4a-multi-frame-handoff.md)

## Scope lock

**TWO-FRAME ADJACENT SEGMENT ONLY.** When `bundle.frames.len() == 2`, build exactly **one** segment (frame[0]→frame[1]). N-1 multi-segment timelines → S1-4c+.

## Artifact boundary

| Property | S1-4b | Not S1-4b |
|----------|-------|-----------|
| Storage | Crate-local `scan-timeline.json` beside `scan-frame-*.json` | Run artifact role / `LocalStore` |
| Consumer | `auv-scan` tests; future S2 read paths | Root `inspect_run`, `inspect_server` |
| Contrast | Directory-level scan wire | [S6b-1 staging](2026-07-03-auv-scan-s6b-scene-state-run-read-handoff.md) run-level `scan-scene-state-input-v0` |

## Wire: `scan-timeline-v0`

**File:** `scan-timeline.json`  
**`schema_version`:** `scan-timeline-v0`

```json
{
  "schema_version": "scan-timeline-v0",
  "segments": [
    {
      "from_frame_id": "frame-0001",
      "to_frame_id": "frame-0002",
      "from_sequence_index": 0,
      "to_sequence_index": 1,
      "motion": {
        "status": "estimated",
        "delta_x": 0,
        "delta_y": 12,
        "confidence": 1.0
      }
    }
  ],
  "diagnostics": []
}
```

**NOTICE(s1-4b):** Not `scan-frame-v0` extension; not run-level artifact; not N-1 multi-segment timeline.

## Diagnostic encoding (no double-encoding)

| Layer | Codes |
|-------|-------|
| Top-level `diagnostics` | Bundle-level only: `insufficient_frames`, `unsupported_frame_count` |
| `segment.motion = Unknown` | Segment motion not decidable (e.g. non-monotonic index in handbuilt bundle) |

Do **not** copy segment Unknown into top-level diagnostics.

## Empty segments persistence

`segments=[]` with non-empty `diagnostics` is a **valid** wire. `write_timeline_artifact` succeeds (not an IO error). Distinguishes:

- missing `scan-timeline.json` on disk
- present file recording bundle constraint failure

## Stable public API

| Symbol | Role |
|--------|------|
| `build_scan_timeline_from_bundle` | `ScanFrameBundle` → `ScanTimelineWire` |
| `write_timeline_artifact` / `read_timeline_artifact` | Crate-local JSON IO (fail-closed schema) |
| `format_scan_timeline_text` | Text markers: `[timeline.segment]`, `[timeline.motion]`, `[timeline.diagnostic]` |

Motion delta algorithm: **only** via existing `estimate_viewport_motion` (`motion.rs`).

## Build rules

> **Superseded by [S9a](2026-07-10-auv-scan-s9a-nframe-adjacent-timeline-handoff.md):** builder now emits N-1 segments when `len >= 2`; table below is historical S1-4b policy.

| `frames.len()` | Result (S1-4b historical) |
|----------------|--------|
| `< 2` | `segments=[]`, diagnostic `insufficient_frames` |
| `== 2` | One segment; motion from `estimate_viewport_motion` |
| `> 2` | `segments=[]`, diagnostic `unsupported_frame_count` |

## Reader vs builder testing

| Path | Test | Behavior |
|------|------|----------|
| Reader | `load_scan_frames_rejects_duplicate_sequence_index_in_directory` | Two JSON files, same `sequence_index` → `DuplicateSequenceIndex` |
| Builder | `build_scan_timeline_preserves_motion_unknown_on_handbuilt_bundle` | Handbuilt bundle, non-monotonic index in frame pair → segment `Unknown`; top diagnostics empty |

Note: After sort-by-index, unique `sequence_index` values are always strictly increasing; `NonMonotonicSequenceIndex` is defensive in `reader.rs`. Non-monotonic **motion** is exercised on the builder path via handbuilt bundles.

## Tests added (9)

`build_scan_timeline_matches_two_frame_manifest`, `write_read_timeline_artifact_roundtrip`, `read_timeline_artifact_rejects_unknown_schema_version`, `write_timeline_artifact_allows_empty_segments_with_diagnostics`, `build_scan_timeline_insufficient_frames`, `build_scan_timeline_unsupported_frame_count`, `load_scan_frames_rejects_duplicate_sequence_index_in_directory`, `build_scan_timeline_preserves_motion_unknown_on_handbuilt_bundle`, `format_scan_timeline_text_includes_markers`

## Validation

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan timeline
cargo test -p auv-scan
git diff --check
```

## Deferred (S1-4c+ / other lanes)

| Item | Notes |
|------|-------|
| N-1 multi-segment timeline | **Landed in [S9a](2026-07-10-auv-scan-s9a-nframe-adjacent-timeline-handoff.md)** |
| Root `inspect_run` timeline block | B-line / run_read slice |
| `motion_unstable` via image diff | New fixture + scroll_scan donor |
| Runtime run artifact producer | Not S1-4b |
| `scene_state` behavior change | Unchanged in S1-4b |
