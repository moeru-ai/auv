# AUV Scan S9b: N-frame Adjacent Multi-Segment Tracks Wire — Implementation Handoff

**Date:** 2026-07-10  
**Status:** implemented — `build_scan_tracks_from_bundle` emits N-1 adjacent association segments when `len >= 2` and observations align (`landed proof` for adjacent multi-segment tracks wire; **not** track/identity continuity substrate)  
**Prerequisite:** [S9a N-frame timeline](2026-07-10-auv-scan-s9a-nframe-adjacent-timeline-handoff.md), [S1 bounded contract review](2026-07-05-auv-s1-bounded-contract-graduation-review.md)

## Scope lock

**S9b only:** `scan-tracks-v0` directory-level wire/IO + builder — segment-centric mirror of S9a timeline; per adjacent pair calls `associate_adjacent_frames` on `observations_by_frame`.

**NOT this slice:** association algorithm upgrade; `scene_state` / `associations_for_bundle` behavior change; track-centric rollup; continuity verdict / ID switch policy; `inspect_run` / `inspect_server`; runtime invoke / live capture; run-level artifact role.

## Wire contract (`scan-tracks-v0`)

| Field | Role |
| --- | --- |
| `schema_version` | `"scan-tracks-v0"` |
| `segments` | N-1 `TrackSegmentWire` (adjacent frame pairs) |
| `diagnostics` | Bundle-level `TracksDiagnosticWire` only |

Per segment: `from_*` / `to_*` frame ids + `associations: Vec<AssociationResultWire>` (`linked` \| `new_track` \| `ambiguous_association`).

### Two diagnostic layers (must not collapse)

| Layer | Rust type | Wire location |
| --- | --- | --- |
| **Bundle-level** | `TracksDiagnosticWire` | `ScanTracksWire.diagnostics` — builder policy only |
| **Segment-level** | `AssociationDiagnosticWire` | nested in `AssociationResultWire::AmbiguousAssociation` |

Never copy segment-level diagnostics into top-level `TracksDiagnosticWire`.

### `track_id` semantics

`NOTICE(s9b-track-id):` `track_id` mirrors adjacent label-based projection (`track-{label}` per `association.rs`); **not** a stable cross-segment identity claim. N-1 segments do not assert global track continuity or ID-switch policy.

`NOTICE(s9b-artifact-boundary):` directory-level beside `scan-frame-*.json`; not run artifact role; not scene_state product wire.

## Builder policy (locked precedence)

| Order | Condition | `segments` | `diagnostics` |
| --- | --- | --- | --- |
| 1 | `frames.len() < 2` | `[]` | `[insufficient_frames]` (reuse `timeline::DIAG_INSUFFICIENT_FRAMES`) |
| 2 | `observations_by_frame.len() != frames.len()` | `[]` | `[observations_frame_mismatch]` |
| 3 | else | `N-1` | `[]` |

When adjacent pairs do not exist, `insufficient_frames` wins over mismatch.

## scene_state boundary

`associations_for_bundle` still uses **last pair only** (S5a scope). `NOTICE(s9b-deferral)` in `scene_state.rs` — N-frame durable associations live in `scan-tracks-v0`; scene consumer deferred.

## Fixtures / test hard gates

| Gate | Requirement |
| --- | --- |
| two-frame linked | **required** `tracks/two_frame_linked_v0` manifest-driven test |
| three-frame linked | **required** `tracks/three_frame_linked_v0` (2 segments) |
| four-frame | handbuilt smoke only (`segments.len() == 3`); **no** golden fixture |

## Files touched

| File | Change |
| --- | --- |
| `crates/auv-scan/src/tracks.rs` | Wire types, builder, read/write/format, tests |
| `crates/auv-scan/src/lib.rs` | Bounded re-exports |
| `crates/auv-scan/src/scene_state.rs` | `NOTICE(s9b-deferral)` only |
| `crates/auv-scan/tests/fixtures/scan/tracks/two_frame_linked_v0/` | Manifest |
| `crates/auv-scan/tests/fixtures/scan/tracks/three_frame_linked_v0/` | Manifest |

## Validation

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
git diff --check
```

## Graduation note

S9b = **landed proof** for N-frame **adjacent association tracks wire** (`scan-tracks-v0`). S2 **tracks substrate** row remains **`hold`** (no rollup / no scene consumer / no runtime producer / no identity continuity verdict).
