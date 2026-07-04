# AUV Scan S8a: Durable Coverage Wire — Implementation Handoff

**Date:** 2026-07-07  
**Status:** implemented — `scan-coverage-v0` crate-local wire + IO (`landed proof` for wire cluster only; S3 substrate stage remains `partial`)  
**Prerequisite:** [S4 lifecycle evaluator](2026-07-02-auv-scan-s4-lifecycle-evaluator-handoff.md), [S-line graduation review](2026-07-04-auv-s-line-graduation-review.md)

## Scope lock

**S8a only:** wire types, `coverage_view_to_wire` projection, `write_coverage_artifact` / `read_coverage_artifact`, golden fixtures, hermetic tests.

**NOT this slice:** runtime producer, `scene_state` consume, `inspect_server`, TERMS, evaluator semantic changes.

## Artifact boundary

| Property | S8a | Not S8a |
| --- | --- | --- |
| Storage | Crate-local `scan-coverage.json` beside `scan-frame-*.json` | Run artifact role / `LocalStore` staging |
| Consumer | `auv-scan` tests; future S8b read paths | Root `inspect_run`, `inspect_server` |
| Contrast | Directory-level coverage wire | [S7](2026-07-06-auv-scan-s7-invoke-frame-producer-handoff.md) run-level frame staging |

```
NOTICE(s8a-artifact-boundary):
- crate-local directory artifact beside scan-frame-*.json
- NOT run-level artifact
- NOT runtime-staged by S8a
- NOT scene_state durable product
- NOT inspect_server consumer in this slice
- bounded crate-local coverage artifact only
```

## Projection iron law

`coverage_view_to_wire` is a **projection only**:

- Input: existing in-memory `CoverageView`
- Injects `schema_version`
- **Does not** accept `ScanFrameBundle` + associations
- **Does not** recompute coverage (evaluator stays in [`coverage.rs`](../../crates/auv-scan/src/coverage.rs))

## Wire: `scan-coverage-v0`

**File:** `scan-coverage.json`  
**`schema_version`:** `scan-coverage-v0`

### Complete (stable linked track)

```json
{
  "schema_version": "scan-coverage-v0",
  "entries": [
    {
      "track_id": "track-widget",
      "last_seen_frame_id": "frame-0002",
      "observation_count": 2
    }
  ],
  "open_uncertainty_codes": [],
  "negative_evidence": [],
  "completeness": {
    "status": "complete"
  }
}
```

### Incomplete (ambiguous association)

```json
{
  "schema_version": "scan-coverage-v0",
  "entries": [],
  "open_uncertainty_codes": ["ambiguous_association"],
  "negative_evidence": [],
  "completeness": {
    "status": "incomplete",
    "reason": "open uncertainties or negative evidence remain"
  }
}
```

### Incomplete (no new observation)

```json
{
  "schema_version": "scan-coverage-v0",
  "entries": [],
  "open_uncertainty_codes": [],
  "negative_evidence": [
    {
      "code": "no_new_observation",
      "after_frame_id": "frame-0002"
    }
  ],
  "completeness": {
    "status": "incomplete",
    "reason": "open uncertainties or negative evidence remain"
  }
}
```

**`completeness` shape:** `#[serde(tag = "status", rename_all = "snake_case")]` — `complete` | `incomplete { reason }`.

**NOTICE(s8-bounded-coverage):** Two-frame / small-bundle evaluator semantics only; not streaming ledger graduation.

## Reader validation (donor parity)

Same strength as [`read_timeline_artifact`](../../crates/auv-scan/src/timeline.rs):

- Read file → parse JSON → require `schema_version` → version match → `serde_json::from_value`
- **No** extra business semantic validation in S8a

## Golden fixture discipline

Path: `crates/auv-scan/tests/fixtures/scan/coverage/<scenario>/golden/scan-coverage.json`

| Fixture | Fixed pipeline (sole input source) |
| --- | --- |
| `coverage_stable_v0` | `two_frame_v0` + widget observations → `associate_adjacent_frames` → `build_coverage_view` → `coverage_view_to_wire` |
| `coverage_no_observation_v0` | `two_frame_v0` + empty associations |
| `coverage_ambiguous_v0` | `two_frame_v0` + duplicate-label ambiguous observations |

Golden JSON is **only** committed output of the fixed pipeline (see test `coverage_view_to_wire_matches_golden_*`). No hand-written / runtime dual-track goldens.

**Regeneration:** `cargo test -p auv-scan coverage_golden_regenerate -- --ignored`

## Public API (`lib.rs` minimal export)

- `SCAN_COVERAGE_SCHEMA_VERSION`
- `SCAN_COVERAGE_ARTIFACT_FILE_NAME`
- `ScanCoverageWire`
- `CoverageArtifactError`
- `coverage_view_to_wire`
- `write_coverage_artifact`
- `read_coverage_artifact`

## Graduation language (conservative)

- **`scan-coverage-v0` wire/IO cluster** gains landed-proof evidence on S8a merge
- **S3 coverage ledger as substrate stage** remains `partial` until producer (S8c) + consumer (S8b/S8d) chain lands

## Merge gate

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
git diff --check
```
