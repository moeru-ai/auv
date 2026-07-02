# AUV Scan S6a Scene State Inspect — Handoff

**Date:** 2026-07-03  
**Status:** landed (S6a)

## Naming

- Charter **S5b / L3** is the **old name**; this slice and follow-on docs use **S6a**.
- Substrate **S6** (model cold-path backends: SLAM, 3DGS, etc.) is **unrelated**.

## Role

S6a proves **L3 in-memory consumption** of `SceneStateProduct`: structured text / read-model projection. It is **not** a viewer, durable wire, or `run_read` bridge.

## API (`crates/auv-scan/src/scene_state_inspect.rs`)

| Symbol | Layer | Role |
|--------|-------|------|
| `SceneStateInspect` | L3 | In-memory consumption surface; `product` is **memory-only convenience**, NOT schema/cache |
| `SceneStateListSummary` | L3 | List/badge projection (blocking, tracks, recommendation codes) |
| `build_scene_state_inspect` | L3 | Entry: wraps `build_scene_state_product` + input metadata |
| `summarize_scene_state_inspect` | L3 | Summary projection |
| `format_scene_state_inspect_text` | L3 | Full structured text (section markers, no IO) |

**No `Serialize` / `Deserialize`** on inspect types.

### L2 vs L3

| L2 (`scene_state.rs`) | L3 (`scene_state_inspect.rs`) |
|-----------------------|-------------------------------|
| `build_scene_state_product` | `build_scene_state_inspect` |
| `summarize_scene_state_text` — metadata-only; adds `recommended=[codes]` | `format_scene_state_inspect_text` — full consumption sections |

### Text sections (`format_scene_state_inspect_text`)

Fixed order, marker-prefixed:

1. `[scene.input]`
2. `[scene.readiness]`
3. `[scene.track]`
4. `[scene.recommended]`
5. `[scene.diagnostics]`
6. `[scene.draft_answers]`

### `SceneStateListSummary.has_scene_state`

Always `true` when inspect build succeeds. Reserved for future list aggregation over partial inputs; S6a tests do not focus on this field.

### Crate-private helper

`observations_match_bundle` in `scene_state.rs` (`pub(crate)`) — shared by L2 builder and L3 inspect; **not** re-exported from `lib.rs`.

## Tests

`cargo test -p auv-scan` — 52 prior + 9 S6a inspect tests.

S6a fixture tests assert **consumption projection only** (summary fields, section markers, `inspect.product == direct product`). They do **not** repeat S5a semantic assertions (`assert_scene_expect`).

Merge gate: `inspect_product_matches_direct_build`.

## Non-goals (S6a)

- Durable `scan-scene-state-v0` wire
- `Serialize` on inspect types
- `src/run_read` / `inspect_run` / `inspect_server`
- Viewer / artifact UI drill-down
- Substrate S6 model backends
- S5a composition semantics changes

## Deferred

| Slice | Status |
|-------|--------|
| **S6b** | **Candidate** — `src/scene_state_read.rs` + `inspect_run` text; artifact drill-down; **requires owner sign-off** |

## Related

- [S5a handoff](2026-07-03-auv-scan-s5-scene-state-handoff.md)
- [S5 charter](2026-07-03-auv-scan-s5-scene-state-charter.md)
