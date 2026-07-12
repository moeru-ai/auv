# SceneBridge A3: Prototype Boundary Review

**Date:** 2026-06-30  
**Status:** **owner accepted Package A3-min** — prototype boundary for ViewMemory +
reacquire on NetEase playlist sidebar. Does not approve **SceneBridge A4**
(run-storage migration, promotion, or full spec parity). Session API P14 pause
unchanged.

**Prior work:** [A2 boundary review](2026-06-30-scenebridge-boundary-decision-review.md)
(**Package A accepted**) → [A2 evidence pack](2026-06-30-scenebridge-netease-sidebar-evidence-pack.md)
→ this note locks A3 prototype forks before Rust.

## One-line summary

SceneBridge A3 is the first **implementation** slice on the read-side grounding
lane: minimal `auv-view::memory` + partial anchor reacquisition, wired into
NetEase `playlist select` to replace the rescan-replay NOTICE — under an
**artifact-dir bridge** and **feature gate**, without run-storage integration or
`CandidateRef` promotion.

**Owner answer: Package A3-min** — bridge persistence, `playlist_sidebar`
scope_id, label+section reacquire targets, stages 1/3/5 hermetic + stage 4 live,
`SidebarReacquireAdapter`, command JSON diagnostics only, hermetic proof required.

## Owner freeze block

```text
persistence：artifact-dir ViewMemory JSON bridge（run storage → A4）
scope_id：playlist_sidebar（shipped region name）
id strategy：label + section_hint + anchor_id（no ViewNodeId migration in A3）
reacquire：stages 1/3/5 hermetic + stage 4 live；AX stages 2/6 → A4
parser integration：SidebarReacquireAdapter（no RegionParser/ItemParser extraction）
consumer：playlist select rescan replacement（play --candidate-id inherits）
trace/inspect：PlaylistSelectResult JSON diagnostics only
proof：hermetic required；live optional owner-labeled
feature gate：AUV_NETEASE_VIEW_MEMORY=1（default off until A3e sign-off）
```

### English expansion (for reviewers)

| Statement | Meaning | Evidence |
| --- | --- | --- |
| Artifact-dir bridge | `ViewMemory` JSON written beside `playlist-scan-cache.json` under `--artifact-dir`; not run-storage `view-memory` role yet | [`PLAYLIST_SCAN_CACHE_FILE`](../../../../crates/auv-netease-music/src/lib.rs), [gap card](../evidence/2026-06-30-scenebridge-netease-sidebar/gap-run-storage-bridge.txt) |
| `scope_id = playlist_sidebar` | Memory key matches shipped `ViewRegionRecord.name`, not provisional `netease.playlist_sidebar` alone | [A2 evidence pack](2026-06-30-scenebridge-netease-sidebar-evidence-pack.md) vocabulary table |
| Label+section durable keys | Reacquire targets use `ReacquireTarget::LabelWithSection` + optional `Anchor`; `item_id`/`candidate_id` stay parse-scoped for JSON | [`PlaylistSidebarScan` doc](../../../../crates/auv-netease-music/src/lib.rs) L317–325 |
| Partial reacquire cascade | Stages 1, 3, 5 in `auv-view::memory::reacquire`; stage 4 in live adapter; stages 2, 6 deferred | [anchor-reacquisition-v0](../view-memory/2026-05-29-view-parser-anchor-reacquisition-v0.md) |
| SidebarReacquireAdapter | NetEase calls existing `parse_sidebar_viewport` / region helpers — no `RegionParser` trait extraction | [layer-contracts-v0](../view-memory/2026-05-29-view-parser-layer-contracts-v0.md) (aspirational vs shipped) |
| Consumer = playlist select NOTICE | Replace scroll-replay in [`playlist.rs` L353–357](../../../../crates/auv-netease-music/src/commands/playlist.rs); `playlist play --candidate-id` inherits via `run_playlist_select_resolved` | [`run_playlist_play_candidate_id`](../../../../crates/auv-netease-music/src/commands/playlist.rs) |
| Command JSON inspect | Reacquire `strategy_used`, `observation_count`, stale reason on `PlaylistSelectResult` — not `view.reacquire.*` spans | [inspect-viewer-v0](../view-memory/2026-05-29-view-parser-inspect-viewer-v0.md) deferred |
| Feature gate default off | `AUV_NETEASE_VIEW_MEMORY=1` enables path; rescan fallback when off or on failure | A3 implementation handoff |
| Spec crate mapping | `auv-example-netease-playlist` in view-parser specs = shipped **`auv-netease-music`** | [example-placement-v0](../view-memory/2026-05-29-view-parser-example-placement-v0.md) |

## Slice classification

| Item | Value |
| --- | --- |
| This note (A3 prototype boundary) | **docs-only** |
| A3a–A3d implementation | **owner-approved feature** |
| Not | bug fix, test-only, narrow refactor (whole lane) |

## Problem / why now

A2 locked Package A and curated hermetic evidence but **explicitly froze** Rust.
The highest-value consumer debt is the
[`NOTICE(netease-playlist-select-reacquire)`](../../../../crates/auv-netease-music/src/commands/playlist.rs)
path: `playlist select` replays full scan scroll count instead of reading
persisted anchors.

Without an A3 boundary, implementation risks:

- blocking on run-storage integration NetEase does not use today,
- implementing full 6-stage reacquire + trace spans before a working prototype,
- extracting parser layer traits in parallel with memory work,
- conflating reacquire success with `CandidateRef` promotion.

## Evidence / inventory (A3 entry)

| Gap | Location | A3-min response |
| --- | --- | --- |
| No `auv-view::memory` | [`crates/auv-view/src/lib.rs`](../../../../crates/auv-view/src/lib.rs) | A3a–A3b types + writer/reader |
| Rescan replay | `playlist.rs` L353–357 | A3d reacquire branch |
| `playlist-scan-cache.json` only | [`cli.rs` `write_playlist_scan_cache`](../../../../crates/auv-netease-music/src/cli.rs) | Also write `view-memory-playlist_sidebar.json` on `playlist ls` |
| No `RunId` in netease crate | grep netease crate | Synthetic `source_run_id` + NOTICE at write site |
| Parser traits spec'd, not shipped | layer-contracts-v0 | `SidebarReacquireAdapter` bridge |
| A1 Q4 inspect contract | A1 charter #4 | Partial: product JSON diagnostics |
| A1 Q5 cross-app | A1 charter #5 | Deferred to A4+ |

### Normative specs (shape reference)

- [view-memory-v0](../view-memory/2026-05-29-view-parser-view-memory-v0.md) — type, lifecycle, done criteria #1–7
- [anchor-reacquisition-v0](../view-memory/2026-05-29-view-parser-anchor-reacquisition-v0.md) — cascade, `ReacquireOutcome`, done criteria #1–7
- [A2 curated fixtures](../evidence/2026-06-30-scenebridge-netease-sidebar/) — hermetic sidebar vectors

## Options analysis — P1 through P8

### P1 Persistence backend

| Package A3-min (recommend) | Package A3-full (defer) |
| --- | --- |
| Write/read `ViewMemory` JSON under `--artifact-dir` beside scan cache; run-storage migration → **A4** | Block until AUV run storage + `view-memory` artifact role |

**Reviewer recommendation:** A3-min. NetEase CLI has no `RunId`; run-storage-first is a cross-layer slice.

### P2 `scope_id` canonicalization

| A3-min | A3-full |
| --- | --- |
| **`playlist_sidebar`** (shipped region name) | Migrate to `netease.playlist_sidebar` everywhere |

**Reviewer recommendation:** A3-min.

### P3 ID strategy

| A3-min | A3-full |
| --- | --- |
| `LabelWithSection` + optional `Anchor`; parse-scoped `item_id`/`candidate_id` for JSON only | Content-derived `ViewNodeId` migration first |

**Reviewer recommendation:** A3-min. Content-derived IDs → **A4**.

### P4 Reacquire depth

| A3-min | A3-full |
| --- | --- |
| Stages **1, 3, 5** hermetic; **4** live; defer **2, 6** | Full 6-stage + all thresholds |

**Reviewer recommendation:** A3-min.

### P5 Parser integration

| A3-min | A3-full |
| --- | --- |
| **`SidebarReacquireAdapter`** in `auv-netease-music` | Extract `RegionParser`/`ItemParser` into `auv-view::parsers` |

**Reviewer recommendation:** A3-min. `TODO(parser-layer-traits-a4)` at adapter.

### P6 Primary consumer

| A3-min | A3-full |
| --- | --- |
| **`run_playlist_select_resolved`** NOTICE replacement; memory write on **`playlist ls`** | Broader command surface rewrite |

**Reviewer recommendation:** A3-min.

### P7 Trace / inspect

| A3-min | A3-full |
| --- | --- |
| **`PlaylistSelectResult`** JSON fields | `view.reacquire.*` spans + `list_view_memory_writes` |

**Reviewer recommendation:** A3-min. Deferred A3-full surfaces and proof tiers →
[A5 inspect identity proof charter](2026-06-30-scenebridge-inspect-identity-proof-charter.md)
(docs freeze; not implementation).

### P8 Proof class

| A3-min | A3-full |
| --- | --- |
| **Hermetic required**; optional one `live`-labeled run | Live mandatory before merge |

**Reviewer recommendation:** A3-min.

## Owner decision packages

### Package A3-min — Prototype Package A (**accepted**)

**Owner: Package A3-min accepted** — 2026-06-30.

```text
A3-A  artifact-dir ViewMemory JSON bridge
A3-B  scope_id playlist_sidebar
A3-C  label + section_hint + anchor_id targets
A3-D  reacquire stages 1/3/5 + live stage 4
A3-E  SidebarReacquireAdapter (no trait extraction)
A3-F  playlist select consumer + playlist ls memory write
A3-G  command JSON diagnostics only
A3-H  hermetic proof required; live optional
A3-I  AUV_NETEASE_VIEW_MEMORY=1 gate (default off)
```

### Package A3-full — Spec-complete prototype — **not accepted**

```text
A3-A  run-storage view-memory artifacts required first
A3-B  netease.playlist_sidebar scope migration
A3-C  content-derived ViewNodeId before reacquire
A3-D  full 6-stage cascade + trace spans
A3-E  RegionParser/ItemParser trait extraction
A3-F  inspect read API + memory_write spans
A3-G  live desktop proof mandatory
```

## Anti-misread rules (frozen)

1. **A3 ≠ full view-parser spec compliance** — bridge persistence + partial cascade are intentional.
2. **A3 ≠ CandidateRef** — reacquire returns bounds for click; promotion stays A4+.
3. **`ViewMemory` in artifact-dir ≠ run-storage `view-memory` role** — bridge until A4.
4. **Fallback rescan preserved** — gate off or reacquire failure must not break today's CLI.
5. **`playlist ls --json`** produces `MatchRef` + scan cache (not bare `playlist --json`).
6. **Spec `auv-example-netease-playlist` = shipped `auv-netease-music`** — see mapping table above.
7. **A3 reopen does not unlock session API, catalog, or TERMS** unless separately named.
8. **Synthetic `source_run_id` is not a real run record** — placeholder until A4 run recording.
9. **A3-min reacquire ≠ durable `anchor_id` alone** — label+section is the primary cross-run key.
10. **Hermetic tests ≠ live NetEase proof** — live evidence is optional and labeled.
11. **Removing NOTICE / default-on gate requires A3e owner sign-off** — not automatic on merge.
12. **ViewMemory v0 done criteria #5 (memory_write span) deferred** — command JSON only in A3-min.

## Explicit non-goals (SceneBridge A3)

This note does **not** approve:

- New `SceneTarget` / `SceneIdentity` in [`contract.rs`](../../../../src/contract.rs)
- `auv-scenebridge` crate
- Root `src/catalog.rs` NetEase `command_id`
- Session API / P14 reopen, MCP merge, `candidate-action` expansion
- `CandidateRef` / surface-analyze promotion wiring
- QQ Music second donor
- Full 6-stage reacquire + `view.reacquire.*` trace spans (A3-full)
- Run-storage `view-memory` artifact role for **one consumer** (`playlist ls --store-root`) — **landed A7-min**; inspect/trace read API → **A8**
- Content-derived `ViewNodeId` migration (A4)
- `TERMS_AND_CONCEPTS.md` update (unless owner names TERMS slice)

## Reopen triggers for A4

| Trigger | Unlocks | Does **not** auto-unlock |
| --- | --- | --- |
| Owner names **SceneBridge A8** inspect/trace slice | `list_view_memory_writes`, `view.parse.memory_write` spans | Catalog, session API |
| **A7-min landed** (`playlist ls --store-root`) | `view-memory` artifact role + real `source_run_id` for one consumer | Default-on store-root, full invoke graduation |
| Owner names **content-derived ViewNodeId** slice | ir-shapes ID migration + stage 1 id stability | Promotion gate |
| Owner names **promotion / CandidateRef** slice | surface-analyze wiring from view memory | Root catalog without consumer |
| Owner signs **Package A3-full** fork | Revisit P1/P4/P7 | A4 by itself |

## A1 open questions — partial resolution

| # | Question (A1) | A3 resolution |
| --- | --- | --- |
| 4 | Inspect contract — artifacts/trace for identity decisions? | **Partial → A5 freeze:** [A5 charter](2026-06-30-scenebridge-inspect-identity-proof-charter.md); trace/inspect API impl → future slice |
| 5 | Cross-app scope? | **Deferred** — NetEase `playlist_sidebar` namespace only |

## Relationship to A2 / view-parser / API pause

- **A2 Package A** pins descriptor, locus, promotion gate, CLI binding — unchanged.
- **A3** implements the smallest read-side prototype A2 deferred.
- **view-memory-v0** + **anchor-reacquisition-v0** are normative references; A3-min documents explicit gaps.
- **P14** pause unchanged.

## Validation

```sh
git diff --check
rg -n "SceneBridge|ViewMemory|reacquire" docs/ai/references/2026-06-30-auv-scenebridge-a3-*
cargo test -p auv-view memory
cargo test -p auv-netease-music view_parsers::sidebar
```

Expected: boundary + handoff docs cite P1–P8 and Package A3-min; hermetic memory/reacquire tests pass; feature gate off preserves rescan behavior.

## Related

- [A3 implementation handoff](2026-06-30-scenebridge-closure.md)
- [A2 boundary decision review](2026-06-30-scenebridge-boundary-decision-review.md)
- [A2 NetEase sidebar evidence pack](2026-06-30-scenebridge-netease-sidebar-evidence-pack.md)
- [A1 design charter](2026-06-30-scenebridge-design-charter.md)
- [view-memory-v0](../view-memory/2026-05-29-view-parser-view-memory-v0.md)
- [anchor-reacquisition-v0](../view-memory/2026-05-29-view-parser-anchor-reacquisition-v0.md)
- [P14 API pause](../session-api/2026-06-30-session-api-closeout.md)
