# AUV Scan S7: Invoke Frame Producer — Implementation Handoff

**Date:** 2026-07-06  
**Status:** landed — `scan.frame` fixture-first invoke path writes bounded scan artifacts into runs  
**Prerequisite:** [S1 slice 2 producer handoff](2026-07-02-auv-scan-s1-slice2-producer-handoff.md), [S-line graduation review](2026-07-04-auv-s-line-graduation-review.md)

## Boundary

**Owning surface:** `crates/auv-cli-invoke` only (Phase 1). **No** changes to `crates/auv-scan` wire/producer in this slice.

**Command:** `scan.frame --fixture-dir <PATH>` — hermetic MVP via `auv_scan::produce_frame_from_fixture_dir`.

**Invoke namespace:** `InvokeNamespace::Scan` (`scan` group).

## Artifact roles (`ProducedArtifact`)

`ProducedArtifact` has **only** `kind`, `source_path`, `preferred_name`, `note`. **No `mime` field.**

| `kind` (→ run `role`) | Source | MIME (store-inferred) |
| --- | --- | --- |
| `scan-frame-v0` | producer JSON path | `application/json` (`.json`) |
| `scan-frame-image` | producer PNG path | `image/png` (`.png`) |

Run staging renames files to `artifacts/artifact_NNNN_<sanitized-preferred-name>.<ext>` — tests must assert **role + staged path**, not producer temp filenames.

## NOTICE(s7-temp-artifact-lifetime)

Producer `out_dir` (temp directory) is used only during `produce_frame_from_fixture_dir`. Before the handler returns, JSON/PNG are **copied** to persistent invoke staging paths (same pattern as `window.capture`) so `record_produced_artifacts` can read `source_path` after the producer `TempDir` drops.

## NOTICE(s7-single-frame-only)

Phase 1 uses `produce_frame_from_fixture_dir` only. `produce_frames_from_fixture_dir` exists in `auv-scan` for multi-frame batches — **deferred** to a follow-up owner slice.

## Non-goals (this slice)

- No `scan-frame-v0` wire changes
- No live window capture (`produce_frame_from_capture`) — Phase 2, feature-gated
- No multi-frame invoke producer
- No `run_read` / `inspect_server` consumption of `scan-frame-v0`
- No `scan-scene-state-input-v0` runtime writer
- **No automatic graduation:** landing S7 proves invoke can emit bounded frame artifacts; **does not** graduate runtime producer lane or whole-line substrate (parent review `hold` has additional causes: durable S3–S5, read-side, multi-frame continuity)

## Merge gate

```sh
cargo fmt --check
cargo check -p auv-cli-invoke
cargo test -p auv-cli-invoke
cargo test -p auv-scan
git diff --check
```

## Tests

| Test | Assert |
| --- | --- |
| `scan_frame_from_fixture_dir_stages_artifacts` | `invoke_recorded` → roles `scan-frame-v0` + `scan-frame-image`; staged paths exist; `read_frame_artifact` + `validate_wire` on staged JSON |
| `scan_frame_requires_fixture_dir` | missing `--fixture-dir` errors |
| `scan_frame_dry_run` | dry-run produces no artifacts |
| Registry / help | `scan.frame` registered; `InvokeNamespace::Scan` |

Fixture: `crates/auv-scan/tests/fixtures/scan/temporal/single_frame_v0/`

## Related

- Parent review runtime evidence row: [2026-07-04](2026-07-04-auv-s-line-graduation-review.md) — *evidence toward* `partial` bridge requires owner sign-off; not auto-applied on merge
