# QQ Music ACP-B1 — Gate + Forced Decisions Handoff

**Date:** 2026-07-06  
**Prerequisite:** [ACP gate](2026-07-05-auv-core-app-command-pack-gate.md), [L8-R2](2026-07-06-auv-core-l8-r2-post-acp-closeout-review.md), NetEase [ACP-1](2026-07-05-auv-netease-music-acp-1-handoff.md) + [ACP-2](2026-07-05-auv-netease-music-acp-2-handoff.md) on `main`

> **Orthogonality:** Pack pass ≠ seam re-proof — see [ACP gate callout](2026-07-05-auv-core-app-command-pack-gate.md#orthogonality-callout-mandatory-in-every-acp-handoff).

## Pack identity

| Field | Value |
|-------|-------|
| App | `com.tencent.QQMusicMac` / crate `auv-qqmusic` |
| Lane | ACP-B — second app **bootstrap** hermetic pack |
| B1 role | Gate + forced decisions only (no product code) |

## Why qqmusic (not a NetEase copy)

- Second **active** app-local Rust crate with typed search/select commands.
- Historical evidence under `docs/ai/references/evidence/*qqmusic*` informs fixture shape; it is **not** a finished scan+select+recording seam like NetEase.
- ACP-B proves **packaging migration** (invoke registry, `--store-root`, `persist_*`, inspect-readable artifact) across apps.

## Gap register (pre-B2)

| Capability | NetEase ACP | qqmusic pre-B2 |
|------------|-------------|----------------|
| `invoke/` + app registry | yes | **no** |
| `recording.rs` + `persist_*` | yes | **no** |
| Hermetic fixture + tests | yes | **no** |
| Live scan artifact + view-memory | yes | **no** (out of scope) |
| `run_search` observe anchor | N/A (playlist scan) | `anchor: None` always |

## ACP pattern contract (B2 must follow)

- App-local `qqmusic_registry()` — **not** `default_registry()`.
- Hermetic proofs use `--fixture-dir` + `--store-root`; no `invoke_recorded` shortcut.
- Docs and code land in **separate commits**.
- Merge gate = `GET /runs` JSON shows artifact role; **viewer hint is not in B2 scope** (B2c defer).
- **Non-goals:** L8b/ATL producer work, view-memory, live CI, `action_resolver` / `candidate-action` in `auv-qqmusic`.

---

## Forced decision 1 — Verification goal

| Option | Meaning | B2 merge gate |
|--------|---------|---------------|
| **A — packaging migration** (default) | One hermetic **act** proof + inspect-readable artifact | **Required** |
| B — symmetric observe+act | Paired observe pack like NetEase | Requires decision 2 Observe-A + extra contract |

### Default (no owner override)

**Goal A — packaging migration.** One hermetic act proof is sufficient for ACP-B success.

### Override register

| Item | Default | Owner override |
|------|---------|----------------|
| Verification goal | A | Record here if owner selects B |

---

## Forced decision 2 — Observe contract

**Code fact:** [`run_search`](../../../crates/auv-qqmusic/src/search.rs) returns `SearchCommandReport { anchor: None }`. `SearchAnchorMatch` appears only on `run_select` / `run_click`. There is **no** observe-only product path that reuses `SearchCommandReport` with a populated anchor.

| Branch | Condition | B2 observe |
|--------|-----------|------------|
| **Observe-A** | Owner approves first proof-only observe schema | New role + wire JSON; not pseudo `command: "search"` with anchor |
| **Observe-defer** (default) | Packaging migration first | **Skip B2 observe**; record named defer `ACP-B2-observe` |

### Default (no owner override)

**Observe-defer.** B2 ships **act-only**; observe is a follow-up slice.

### Override register

| Item | Default | Owner override |
|------|---------|----------------|
| Observe | defer | Observe-A only with explicit proof-only schema approval |

---

## Forced decision 3 — Act path (select vs click)

| Option | Code path | `command` field | Click semantics |
|--------|-----------|-----------------|-----------------|
| **`resultsSelectProof`** (default) | Proof-only schema aligned to `run_select` command identity | `search.results.select` | Single click identity; fixture-only packaging proof |
| `resultsClickProof` | `run_click` + `--anchor` | `search.results.click` | Double click / play-visible-anchor class |

**Forbidden:** `resultsSelectProof` name with double-click fixture semantics.

### Default (no owner override)

| Field | Value |
|-------|-------|
| Invoke ID | `qqmusic.search.resultsSelectProof` |
| RunSpec root span | `auv.qqmusic.search.select` |
| Artifact role | `qqmusic-search-select-result` |
| Fixture | `tests/fixtures/select-proof/hermetic_v0/select-result.json` |
| Test filter | `cargo test -p auv-qqmusic results_select_proof` |

### Override register

| Item | Default | Owner override |
|------|---------|----------------|
| Act path | `resultsSelectProof` | `resultsClickProof` only for play-visible-anchor / double-click alignment |

---

## B2 entry checklist

- [x] B1 forced decisions recorded (this doc)
- [ ] `recording.rs` + `invoke/results_select_proof.rs` + fixture
- [ ] `cargo test -p auv-qqmusic results_select_proof` green
- [ ] `auv-qqmusic invoke` lists `qqmusic.search.resultsSelectProof`
- [ ] B2 handoff with NetEase diff + orthogonality

## Named defers (B1)

| ID | Defer |
|----|-------|
| `ACP-B2-observe` | Observe proof until Observe-A approved |
| `ACP-B2c-viewer` | qqmusic proof viewer hint until both apps act-green + owner re-evaluates |

## References

- Plan: ACP-B second app pack (qqmusic)
- [L8-R2 drift D1](2026-07-06-auv-core-l8-r2-post-acp-closeout-review.md) — legacy `missing_input_action_result` test (R2a)
