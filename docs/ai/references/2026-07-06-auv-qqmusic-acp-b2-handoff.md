# QQ Music ACP-B2 — Bootstrap Act Pack Handoff

**Date:** 2026-07-06  
**Prerequisite:** [ACP-B1 gate/handoff](2026-07-06-auv-qqmusic-acp-b1-gate-handoff.md), [ACP gate](2026-07-05-auv-core-app-command-pack-gate.md)

> **Orthogonality:** This pack does not change L8b seam verdict. Pack pass ≠ seam re-proof.

## Landed (B2 act-first)

| Item | Value |
|------|-------|
| Invoke ID | `qqmusic.search.resultsSelectProof` |
| Registry | `crates/auv-qqmusic/src/invoke/mod.rs` → `qqmusic_registry()` |
| Handler | `invoke/results_select_proof.rs` |
| Recording | `recording.rs` → `persist_search_select_proof` |
| Artifact role | `qqmusic-search-select-result` |
| RunSpec | `auv.qqmusic.search.select` |
| Fixture | `tests/fixtures/select-proof/hermetic_v0/` |
| CLI | `auv-qqmusic invoke …` (sibling to `search`) |

## Verification (owner-approved defaults from B1)

```sh
cargo fmt --check && cargo check
cargo test -p auv-qqmusic results_select_proof
cargo run -p auv-qqmusic -- invoke
git diff --check
```

**Success:** Hermetic **packaging proof** persists a run with `qqmusic-search-select-result` artifact; inspect `GET /runs` JSON readable.

## Bootstrap vs NetEase (not a copy)

| Dimension | NetEase ACP-1/2 | qqmusic ACP-B2 |
|-----------|-----------------|----------------|
| Domain schema | `PlaylistSelectResult`, sidebar scan IR | `SearchSelectProofDocument` (wire aligned with `SearchCommandReport`, not full `run_select` replay fidelity) |
| Observe half | `sidebarScanProof` landed | **Deferred** (`ACP-B2-observe`) |
| View-memory / reacquire spans | ACP-1 select uses controlled reacquire subset | **None** — minimal `persist_*` only |
| Act command | `playlist.select` | `search.results.select` command identity (fixture-only single-click packaging proof) |
| Maturity | Scan artifact + select seam existed | **Bootstrap** — first invoke/recording in crate |

Mechanism reused: app-local registry, `--fixture-dir` + `--store-root`, `RunRecordingBackend::run_recorded_operation`, hermetic fixture tests, exclusion from `default_registry()`.

## Evidence boundary

- This pack proves **app-local invoke + store persistence + inspect-readable artifact**.
- It does **not** prove full-fidelity `run_select` execution replay.
- Current fixture is intentionally minimal and does not mirror every live `SearchCommandReport` step or driver result field.

## Observe defer (`ACP-B2-observe`)

**Not implemented** per B1 Observe-defer default.

- `run_search` never produces anchor; no product observe-only path.
- Follow-up requires B1 Observe-A (proof-only schema) before any `*ObserveProof` invoke.

## Viewer defer (`ACP-B2c-viewer`)

**Not implemented** per plan default.

Re-evaluate when NetEase + qqmusic act proofs are both green in inspect JSON and owner requests unified proof-pack viewer polish.

## Non-goals (confirmed)

- `resultsClickProof` / double-click path (unless B1 override)
- L8b producer / `run_read` classify changes (except R2a isolated legacy test)
- `action_resolver` / `candidate-action` in `auv-qqmusic`
- Root `default_registry()` registration

## Next slices (not approved here)

| Slice | Trigger |
|-------|---------|
| ACP-B2-observe | Owner approves Observe-A schema |
| ACP-B2c viewer | Two-app act green + owner request |
| ACP-C third app | Owner names app |
