# AUV Scan S1 Slice 1: Frame + Artifact Contract — Implementation Handoff

**Date:** 2026-07-02  
**Status:** landed — `scan-frame-v0` wire + hermetic single-frame fixture  
**Prerequisite:** [S0 charter](2026-07-02-auv-scan-s0-charter.md), [S1 temporal core plan](2026-07-02-auv-scan-s1-temporal-core-plan.md) step 1

## Boundary decision (locked)

**Owning crate:** [`crates/auv-scan`](../../../crates/auv-scan)

This slice **resolves** the S1 plan’s deferred owning-crate choice. Alternatives rejected:

| Option | Verdict |
| --- | --- |
| Extend `auv-view` | Reject — view-parser IR / ViewMemory boundary |
| `src/scroll_scan` | Reject — page-level scroll workflow, not cross-frame temporal |
| **`crates/auv-scan`** | **Accept** — S-line code home for temporal scan contracts |

**NOTICE:** Stable crate name = **S-line code ownership** only. This slice **approves
`scan-frame-v0` wire** — not motion, tracks, evidence fusion, diagnostics, or full
temporal stack. Names such as `ViewportTransform`, `EvidenceNode`, `TemporalTrack` in the
S1 plan remain **provisional vocabulary** until a later owner-approved slice.

## Approved wire (slice 1 only)

| Constant | Value |
| --- | --- |
| `SCAN_FRAME_SCHEMA_VERSION` | `"scan-frame-v0"` |

| Type | Fields |
| --- | --- |
| `ScanBounds` | `x, y, width, height: i64` |
| `ScanImageRef` | `file_name`, `width`, `height`, `media_type` |
| `ScanFrame` | `schema_version`, `frame_id`, `sequence_index`, `captured_at_millis`, `window_bounds`, `viewport_bounds: Option<ScanBounds>`, `image` |

**Artifact file name:** `scan-frame-NNNN.json` (4-digit, `sequence_index + 1`). Optional
sibling PNG referenced by `image.file_name`.

## Stable public API (`auv_scan` crate root re-exports)

| Symbol | Role |
| --- | --- |
| `SCAN_FRAME_SCHEMA_VERSION` | Wire schema gate |
| `ScanBounds`, `ScanImageRef`, `ScanFrame` | Wire types |
| `write_frame_artifact(dir, frame)` | Write `scan-frame-NNNN.json` under `dir` |
| `read_frame_artifact(path)` | Parse JSON + schema/bounds validation |
| `frame_artifact_file_name(sequence_index)` | Returns **full** name e.g. `scan-frame-0001.json` |
| `ScanArtifactError` | Typed read/write errors |

**Not stable public API:** `build_frame_from_fixture` — lives in `fixture.rs`, compiled
only under `#[cfg(test)]`, **not** re-exported from `lib.rs`.

## Error model

| `ScanArtifactError` variant | When |
| --- | --- |
| `SchemaMismatch { found }` | Wrong or missing approved `schema_version` |
| `InvalidBounds { field }` | `width` or `height` ≤ 0 on bounds fields |
| `MissingField(&'static str)` | Required JSON field absent or empty (`schema_version`) |
| `Io` | File system errors |
| `Json` | JSON parse failures |

Tests assert **variants** (`matches!(err, …)`), not `Display` strings.

## Hermetic fixture

```text
crates/auv-scan/tests/fixtures/scan/temporal/single_frame_v0/
  manifest.json
  frame-0001.png
  golden/scan-frame-0001.json
```

## Tests (7 — all in-crate `#[cfg(test)]`)

| Test | Assert |
| --- | --- |
| `build_frame_from_fixture_single_frame_v0` | Manifest → `ScanFrame` fields |
| `write_then_read_frame_artifact_roundtrip` | Write/read equality |
| `read_frame_artifact_matches_golden_wire` | Matches golden JSON |
| `read_frame_artifact_rejects_unknown_schema_version` | `SchemaMismatch` |
| `read_frame_artifact_rejects_missing_schema_version` | `MissingField("schema_version")` |
| `read_frame_artifact_rejects_non_positive_bounds` | `InvalidBounds` |
| `frame_artifact_file_name_includes_json_extension` | `"scan-frame-0001.json"` |

## S1 plan pointer

[S1 temporal core plan](2026-07-02-auv-scan-s1-temporal-core-plan.md) step 1 (**Frame +
artifact contract + hermetic single-frame fixture**) is **done** in `auv-scan`. Next
approved slice: step 2 (motion between two frames) — separate commit, separate handoff.

## Non-goals (this slice)

Motion, OCR/detector fusion, temporal tracks, diagnostics, CLI, implicit run recording,
live capture, SceneBridge, ViewMemory, query-aware scan, 3D, inspect API, `scroll_scan` /
`auv-view` changes.

## Validation

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
git diff --check
```

## Related

- [S0 charter](2026-07-02-auv-scan-s0-charter.md)
- [S1 temporal core plan](2026-07-02-auv-scan-s1-temporal-core-plan.md)
- [Scroll scan design](2026-05-21-scroll-scan-design.md)
