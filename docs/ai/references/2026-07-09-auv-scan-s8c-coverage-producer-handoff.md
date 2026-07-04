# AUV Scan S8c: Runtime Coverage Producer — Implementation Handoff

**Date:** 2026-07-09  
**Status:** implemented — fixture-first coverage producer + `scan.coverage` invoke staging (`landed proof` for producer chain; S3 substrate stage remains `partial`)  
**Prerequisite:** [S8a coverage wire](2026-07-07-auv-scan-s8a-coverage-wire-handoff.md), [S8b scene consumer](2026-07-08-auv-scan-s8b-scene-coverage-consumer-handoff.md)

## Scope lock

**S8c only:** `CoverageProducerError`, `produce_coverage_from_fixture_dir`, coverage scenario `manifest.json`, `scan.coverage` invoke → `scan-coverage-v0` run role, producer golden parity, invoke staging smoke.

**NOT this slice:** S8d `inspect_run` / `scene_state_read` durable read; TERMS; `build_coverage_view` semantic changes; bundled `scan.frame` + `scan.coverage`; live capture; N-frame evaluator extension; removing `coverage_wire: None` fallback.

## Producer iron law

**Chain:** `build_coverage_view` (evaluator) → `coverage_view_to_wire` (projection only) → `write_coverage_artifact`.

- `coverage_view_to_wire` must **not** accept bundle/associations and recompute.
- No hand-written coverage wire JSON in producer paths.
- **Do not modify** [`build_coverage_view`](../../../crates/auv-scan/src/coverage.rs) semantics.

## Tightened decisions (D1–D4)

### D1. Error model: `CoverageProducerError` (separate from `ScanProducerError`)

[`ScanProducerError`](../../../crates/auv-scan/src/producer/error.rs) is frame-producer semantics (`MissingImage`, `ZeroImageDimension`, `DuplicateFrameId`, …). Coverage manifest, cross-fixture resolution, observation shape, and artifact IO belong in a **separate** enum.

| Variant | Trigger |
| --- | --- |
| `MissingManifest` | `fixture_dir/manifest.json` absent |
| `InvalidManifest` | JSON parse / required fields missing |
| `InvalidObservationShape` | `observations_by_frame.len()` ≠ bundle frame count |
| `InvalidFixtureLayout` | Cross-fixture resolved path not a directory |
| `FrameProducer(ScanProducerError)` | `produce_frames_from_fixture_dir` failure |
| `Artifact(CoverageArtifactError)` | `write_coverage_artifact` failure |
| `Io` / `Json` | Transparent passthrough |

**No** `InsufficientFrames` / `FrameCountTooLow` — see D2.

### D2. `<2` frames: mirror evaluator, no producer error

[`associations_for_bundle`](../../../crates/auv-scan/src/scene_state.rs): `bundle.frames.len() < 2` → empty `associations`, not an error. Producer association step **mirrors** this:

```rust
let associations = if bundle.frames.len() < 2 {
  Vec::new()
} else {
  let last = bundle.frames.len() - 1;
  associate_adjacent_frames(
    &observations_by_frame[last - 1],
    &observations_by_frame[last],
  )
};
```

### D3. Invoke args: `SCAN_COVERAGE_ARGS` (not `SCAN_FRAME_ARGS`)

`scan.coverage` uses dedicated help:

```rust
pub const COVERAGE_FIXTURE_DIR: ArgSpec = ArgSpec {
  flag: "--fixture-dir",
  value_name: "PATH",
  required: true,
  help: "Directory containing a coverage scenario manifest (manifest.json); frame PNGs are resolved via frame_fixture cross-reference, not stored in this directory.",
};
pub const SCAN_COVERAGE_ARGS: &[ArgSpec] = &[COVERAGE_FIXTURE_DIR];
```

`scan.frame` continues `SCAN_FRAME_ARGS` / single-frame help unchanged.

### D4. Cross-fixture contract

| | `scan.frame --fixture-dir` | `scan.coverage --fixture-dir` |
| --- | --- | --- |
| Directory content | Self-contained manifest + PNG | Coverage scenario `manifest.json` only (+ optional `golden/`) |
| Frame assets | In directory | **Not** in coverage dir; `frame_fixture` cross-reference |

**Resolution (fail-closed):**

1. Read `coverage_fixture_dir/manifest.json` → `frame_fixture` (e.g. `temporal/two_frame_v0`).
2. **Scan fixtures root** = `coverage_fixture_dir.parent().parent()` (layout `.../scan/coverage/<scenario>/`).
3. `frame_fixture_dir = scan_fixtures_root.join(frame_fixture)`.
4. Missing directory → `InvalidFixtureLayout`.
5. `produce_frames_from_fixture_dir` → in-memory bundle; frame JSON/PNG **not** written to coverage `out_dir` (only `scan-coverage.json`).

```
tests/fixtures/scan/
  coverage/coverage_stable_v0/manifest.json   ← --fixture-dir
  temporal/two_frame_v0/                        ← manifest.frame_fixture target
```

Phase 1 bounded: invoke tests use repo `tests/fixtures/scan/coverage/*` layout only.

## API

| Symbol | Visibility | Role |
| --- | --- | --- |
| `CoverageProducerError` | `pub` | Independent producer error enum (D1) |
| `ProducedCoverage` | `pub` | `{ json_path, wire: ScanCoverageWire }` |
| `produce_coverage_from_fixture_dir` | `pub` | Main entry; D4 cross-fixture resolution |

**`lib.rs` exports:** `produce_coverage_from_fixture_dir`, `ProducedCoverage`, `CoverageProducerError`.

## Coverage fixture manifest (input contract)

Path: `crates/auv-scan/tests/fixtures/scan/coverage/<scenario>/manifest.json`

| Scenario | `frame_fixture` | `observations_by_frame` |
| --- | --- | --- |
| `coverage_stable_v0` | `temporal/two_frame_v0` | o0/o1 widget (S8a stable) |
| `coverage_no_observation_v0` | `temporal/two_frame_v0` | `[]` / `[]` |
| `coverage_ambiguous_v0` | `temporal/two_frame_v0` | dup labels (S8a ambiguous) |

```json
{
  "scenario": "coverage_stable_v0",
  "frame_fixture": "temporal/two_frame_v0",
  "observations_by_frame": [ ... ]
}
```

Golden JSON remains under `golden/scan-coverage.json`; producer output must equal S8a goldens.

## Invoke — `scan.coverage`

- Args: `SCAN_COVERAGE_ARGS` / `COVERAGE_FIXTURE_DIR` (D3)
- Dry-run: no artifacts (mirror S7)
- Flow: `TempDir` → `produce_coverage_from_fixture_dir` → **copy** JSON to persistent staging (`NOTICE(s7-temp-artifact-lifetime)`)
- Single `ProducedArtifact`:
  - `kind`: `scan-coverage-v0`
  - `preferred_name`: `scan-coverage.json`
  - `note`: evaluator + projection, not hand-written JSON

**Does not** auto-write coverage into `scan.frame` output directory.

## Consumer boundary (unchanged)

- **No changes** to [`scene_state_read.rs`](../../../src/scene_state_read.rs) — `coverage_wire: None` until S8d.
- **No changes** to S8b consumer branch.
- `read_coverage_artifact_from_scan_dir` stays `#[cfg(test)]` + `pub(crate)` — invoke tests use `read_coverage_artifact(&staged_path)`.
- `TODO(s8c-fallback):` S8d may hydrate from run artifact; this slice does not remove in-memory fallback.

## Tests

### `auv-scan` producer golden parity

For each `coverage_{stable,no_observation,ambiguous}_v0`:

```rust
let produced = produce_coverage_from_fixture_dir(fixture_dir, &out_dir)?;
let golden = read_coverage_artifact(golden_path)?;
assert_eq!(produced.wire, golden);
```

### `auv-cli-invoke` staging smoke

| Test | Assert |
| --- | --- |
| `scan_coverage_from_fixture_dir_stages_artifacts` | `invoke_recorded` → role `scan-coverage-v0`; staged path; wire equals `coverage_stable_v0` golden |
| `scan_coverage_requires_fixture_dir` | missing `--fixture-dir` errors |
| `scan_coverage_dry_run` | dry-run produces no artifacts |
| Registry / help | `scan.coverage` registered; coverage fixture help text |

## Graduation language

- **S8c** = runtime coverage producer helper proof (fixture-first invoke → `scan-coverage-v0` run role)
- **S8d** = inspect durable read — see [S8d handoff](2026-07-10-auv-scan-s8d-inspect-coverage-handoff.md)
- **S3 ledger substrate stage remains `partial`** — in-memory `CoverageView` is still the default substrate; **S8 fixture-first durable coverage chain `landed proof`** (S8a–S8d) ≠ whole S3 graduated
- S8a = wire/IO · S8b = scene consumer · **S8c = producer** · S8d = inspect durable — do not mix lanes
- `producer chain landed proof` ≠ whole S3 graduated

## Merge gate

```sh
cargo fmt --check
cargo check -p auv-scan -p auv-cli-invoke
cargo test -p auv-scan
cargo test -p auv-cli-invoke
git diff --check
```

## Related

- [S7 invoke frame producer](2026-07-06-auv-scan-s7-invoke-frame-producer-handoff.md) — temp staging pattern reused
- [S-line graduation review](2026-07-04-auv-s-line-graduation-review.md) — S3 producer note updated
