# AUV Scan S8b: Scene State Coverage Consumer — Implementation Handoff

**Date:** 2026-07-08  
**Status:** implemented — scene_state durable coverage consumer (`landed proof` for consumer path; S3 substrate stage remains `partial`)  
**Prerequisite:** [S8a coverage wire](2026-07-07-auv-scan-s8a-coverage-wire-handoff.md)

## Scope lock

**S8b only:** `coverage_wire_to_view`, `SceneStateInput.coverage_wire`, `build_scene_state_product` consumer branch, fixture golden mapping, whole-product parity tests, inspect durable smoke.

**NOT this slice:** S8c runtime producer; S8d inspect/run_read read paths; TERMS; `build_coverage_view` semantic changes; `scan-timeline.json` consumption; `scan-scene-state-v0` wire.

## Consumer iron law

**Durable coverage is authoritative only for coverage-derived fields:**

- `SceneStateProduct.coverage`
- `collect_blocking_codes` coverage items (`ambiguous_association`, `no_new_observation`)
- `visibility_for_track` paths reading `coverage.negative_evidence`

**Not whole-product driven:** associations still computed; `last_seen` / `latest_observation_present` still from observations.

`coverage_wire_to_view` is **inverse projection only** — no `bundle + associations`, no `build_coverage_view`.

## API

| Symbol | Visibility | Role |
| --- | --- | --- |
| `coverage_wire_to_view` | `pub(crate)` | `ScanCoverageWire` → `CoverageView` |
| `read_coverage_artifact_from_scan_dir` | `pub(crate)` | `dir/scan-coverage.json` sugar |
| `SceneStateInput.coverage_wire` | `pub` | `Some` = durable path; `None` = in-memory legacy |

**No new `lib.rs` re-exports** for `coverage_wire_to_view` (S8a `read_coverage_artifact` remains the public IO surface).

## Construction sites

| Location | `coverage_wire` |
| --- | --- |
| `scene_fixture_support::scene_input_from_fixture` | parity-covered scenes = `Some(read S8a golden)`; other scenes = `None` |
| `scene_state` tests L628, L665 | `None` |
| `src/scene_state_read.rs` | `None` (legacy until S8d) |

## Fixture mapping (S8a golden sole source)

| Scene fixture | S8a golden |
| --- | --- |
| `scene_stable_v0` | `coverage_stable_v0` |
| `scene_stale_v0` | `coverage_no_observation_v0` |
| `scene_ambiguous_v0` | `coverage_ambiguous_v0` |
| `scene_lost_v0` / `scene_lifecycle_bad_evidence_v0` / `scene_missing_observations_v0` | none in S8b (`coverage_wire: None`) |

## Parity gate (whole `SceneStateProduct`)

For `scene_stable_v0`, `scene_stale_v0`, `scene_ambiguous_v0` (lifecycle `null`):

`build_scene_state_product({ coverage_wire: None, .. }) == build_scene_state_product({ coverage_wire: Some(golden), .. })`

Covers visibility → tracks chain, not just `coverage` / blocking / readiness.

## Inspect smoke

`build_scene_state_inspect` + `coverage_wire: Some(golden)` — `inspect.product` equals direct `build_scene_state_product`.

## Graduation language

- **S8b** = scene_state consumer helper proof (coverage-derived path)
- **S3 substrate stage remains `partial`** until S8c producer + S8d inspect chain

## Merge gate

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
cargo test scene_state_read
git diff --check
```
