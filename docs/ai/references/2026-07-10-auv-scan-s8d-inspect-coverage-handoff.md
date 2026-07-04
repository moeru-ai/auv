# AUV Scan S8d: Inspect Durable Coverage — Implementation Handoff

**Date:** 2026-07-10  
**Status:** implemented — `scene_state_read` hydrates run `scan-coverage-v0` into inspect (`landed proof` for inspect durable read; **S3 ledger substrate stage remains `partial`**)  
**Prerequisite:** [S8a coverage wire](2026-07-07-auv-scan-s8a-coverage-wire-handoff.md), [S8b scene consumer](2026-07-08-auv-scan-s8b-scene-coverage-consumer-handoff.md), [S8c coverage producer](2026-07-09-auv-scan-s8c-coverage-producer-handoff.md)

## Scope lock

**S8d only:** run-level `scan-coverage-v0` discovery/read in `scene_state_read`, `coverage_wire` hydration, `[scene.coverage]` inspect text line, hermetic parity tests through `inspect_run`.

**NOT this slice:** `inspect_server` / viewer; `run_read.rs`; bundled `scan.frame` + `scan.coverage`; live capture; `build_coverage_view` semantic changes; `TERMS_AND_CONCEPTS`; whole S3 substrate graduation; whole S-line `hold` lift.

## Read iron law

**Chain:** run artifact `scan-coverage-v0` → `read_coverage_artifact` (S8a public IO) → `SceneStateInput.coverage_wire` → S8b consumer branch in `build_scene_state_product`.

- Root crate does **not** call `coverage_wire_to_view` or recompute coverage.
- Explicit bad/malformed staged coverage → `Unsupported` (fail-closed), not silent in-memory fallback.

## Multi-artifact policy (coverage)

| Count of `scan-coverage-v0` JSON artifacts | Behavior |
| --- | --- |
| 0 | `coverage_wire: None` → in-memory `build_coverage_view` fallback |
| 1 | `read_coverage_artifact(path)` → `coverage_wire: Some(wire)` |
| >1 | `Unsupported { reason: "multiple scan-coverage-v0 artifacts" }` |

Scene input policy (`scan-scene-state-input-v0`) unchanged from [S6b-1](2026-07-03-auv-scan-s6b-scene-state-run-read-handoff.md).

## API / symbols

| Symbol | Location | Role |
| --- | --- | --- |
| `SCAN_COVERAGE_ARTIFACT_ROLE` | `coverage_artifact.rs` | Run artifact role constant (`scan-coverage-v0`) |
| `CoverageInspectSource` | `scene_state_inspect.rs` | L3 metadata: `InMemory` / `Durable` |
| `SceneStateInspect.coverage_source` | `scene_state_inspect.rs` | Set from `input.coverage_wire.is_some()` |
| `resolve_coverage_wire_for_run` | `scene_state_read.rs` (private) | D2/D3 policy |
| `build_scene_state_inspect_for_run` | `scene_state_read.rs` | Hydrates coverage before S6a inspect build |

## Inspect text

`format_scene_state_inspect_text` emits:

```text
[scene.coverage] source=durable|in_memory entry_count=N
```

`coverage_source` lives on L3 `SceneStateInspect` only — **not** on `SceneStateProduct`.

## Fallback boundary

```text
NOTICE(s8d-fallback-boundary): in-memory build_coverage_view fallback remains when run has
zero scan-coverage-v0 artifacts; durable wire is authoritative when exactly one artifact is present.
```

(See `producer/coverage.rs` module header.)

## Tests

| Test | Assert |
| --- | --- |
| `build_scene_state_inspect_for_run_with_durable_coverage_{stable,stale,ambiguous}` | Durable inspect product ≡ in-memory (S8b parity scenes) |
| `build_scene_state_inspect_for_run_present` | `source=in_memory` when no coverage artifact |
| `build_scene_state_inspect_for_run_unsupported_multiple_coverage_artifacts` | Multiple coverage → `Unsupported` |
| `build_scene_state_inspect_for_run_unsupported_bad_coverage_schema` | Bad schema → `Unsupported` |
| `inspect_run_includes_durable_coverage` | `inspect_run` text contains `[scene.coverage] source=durable` |

## Graduation language

- **S8d** = inspect durable read helper proof (`scene_state_read` + `inspect_run`)
- **S8 fixture-first durable coverage chain** (S8a wire → S8b scene consumer → S8c invoke producer → S8d inspect read) = **`landed proof`**
- **S3 ledger substrate stage remains `partial`** — in-memory `CoverageView` is still the default substrate
- **S3–S5 product stack** and **whole S-line** verdicts unchanged (`hold` / `partial` per [S-line graduation review](2026-07-04-auv-s-line-graduation-review.md))
- S8 line closed ≠ S-line `hold` lifted; fixture-first chain landed proof ≠ whole S3 graduated

## Merge gate

```sh
cargo fmt --check
cargo check
cargo test -p auv-scan
cargo test scene_state_read
cargo test -p auv-cli-invoke scan_coverage
git diff --check
```
