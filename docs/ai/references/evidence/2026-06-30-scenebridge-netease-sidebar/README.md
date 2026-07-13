# NetEase Playlist Sidebar SceneBridge Evidence

**Date:** 2026-06-30

This folder contains the curated **hermetic** evidence pack for SceneBridge A2:

`parse → reconstruct → projection → MatchRef → (gap) ViewMemory / reacquire`

It is intentionally selective. It does **not** mirror a live `.auv/` runtime
directory and contains **no production screenshots**.

**Parent pack:** [A2 NetEase sidebar evidence pack](../../2026-06-30-auv-scenebridge-a2-netease-sidebar-evidence-pack.md)

## Included files

- `hermetic-reconstruct-sidebar-synthetic.json`
  - Redacted projection excerpt authored from
    `reconstruct_sidebar_groups_items_under_carried_section` test vectors
    ([`tests.rs`](../../../../../crates/auv-netease-music/src/view_parsers/sidebar/tests.rs) L7–54)
- `match-ref-vocabulary.json`
  - Example `MatchRef` JSON plus field glossary for agent-facing CLI output
- `gap-view-memory-and-reacquire.txt`
  - Curated NOTICE + spec pointers for ViewMemory / reacquire debt
- `gap-run-storage-bridge.txt`
  - artifact-dir vs run-storage seam for A3-min bridge
- `view-memory-roundtrip-synthetic.json`
  - Example `ViewMemory` JSON for hermetic serde / round-trip tests
- `reacquire-target-fixtures.json`
  - Target cases for A3c reacquire cascade tests
- `live/` (`proof_class: live`)
  - [A6 sign-off protocol](live/README.md), [`SIGNOFF.md`](live/SIGNOFF.md), [`transcript.txt`](live/transcript.txt)
  - A6c-3 live case JSON attached; [`examples/`](live/examples/) are structure exemplars only

**A3 docs:** [prototype boundary review](../../2026-06-30-auv-scenebridge-a3-prototype-boundary-review.md),
[implementation handoff](../../2026-06-30-auv-scenebridge-a3-implementation-handoff.md)

**A5 field tiers:** [inspect identity proof charter](../../2026-06-30-auv-scenebridge-a5-inspect-identity-proof-charter.md)

**A6 live closure:** [live evidence closure](../../scenebridge/2026-06-30-scenebridge-closure.md) (**PARTIAL** @ A6c-3 — baseline unblocked; Case B open)

## What this folder proves

- Two-viewport sidebar reconstruction yields 2 sections and 3 playlist items in
  the canonical hermetic fixture.
- `MatchRef` field names are the de facto scene-target vocabulary for the product
  CLI today.
- ViewMemory and reacquire ship in A3-min behind `AUV_NETEASE_VIEW_MEMORY=1`;
  playlist select falls back to rescan replay when the gate is off or reacquire misses.

## What this folder does not prove

- Live NetEase UI capture or OCR on a real desktop session
- Cross-run durable `anchor_id` (parse-scoped until ViewMemory lands)
- QQ Music or other app grounding (separate evidence paths)
- Session API, MCP, or root catalog command binding
