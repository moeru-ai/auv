# SceneBridge A2: NetEase Playlist Sidebar Evidence Pack

**Date:** 2026-06-30  
**Status:** hermetic curated evidence — no live desktop runs in this slice.

**Boundary:** [A2 boundary decision review](2026-06-30-auv-scenebridge-a2-boundary-decision-review.md)
(**Owner: Package A accepted**)

## Scope

This pack records what the NetEase **playlist sidebar** chain proves today for
SceneBridge grounding — and what remains gap until A3:

```text
view parse (per viewport)
  → reconstruct_playlist_sidebar (merge / carry / dedup)
  → stable sidebar item identity fields (projection + anchors)
  → agent-callable MatchRef (product CLI JSON)
  → (gap) ViewMemory persist + reacquire
  → (gap) surface-analyze promotion → CandidateRef
  → playlist play --candidate-id / playlist select <label> (rescan replay today)
```

The purpose is to curate **existing** hermetic tests and specs into a durable
reference — not to capture new live screenshots or `.auv` runtime dirs.

## Grounding vocabulary table

Stable vs ephemeral fields for NetEase playlist sidebar (maps A1 questions to
concrete shapes):

| Concept | Stable across refocus/layout? | Source today | Maps to |
| --- | --- | --- | --- |
| `app_bundle_id` | yes (app identity) | scan `app` context | ViewMemory key ([view-memory-v0](2026-05-29-view-parser-view-memory-v0.md)) |
| `scope_id` / `projection_id` | yes (surface namespace, **provisional**) | `netease.playlist_sidebar` in [view-memory-v0](2026-05-29-view-parser-view-memory-v0.md) / [ir-shapes-v0](2026-05-29-view-parser-ir-shapes-v0.md); shipped region name `playlist_sidebar` | ViewMemory scope |
| `anchor_id` | yes within memory scope (target) | `ViewAnchor` on reconstruction | `MatchRef.anchor_id` |
| `item_id` / `section_id` | mostly stable post-merge | `PlaylistSidebarProjection` | `MatchRef.item_id`, `MatchRef.section_id` |
| `label` | display; normalize for match | OCR text | keyword filter in [`output.rs`](../../crates/auv-netease-music/src/output.rs) |
| `candidate_id` | parse-scoped | optional on `MatchRef` | not yet durable `CandidateRef` |
| Window bounds / scroll offset | **ephemeral** | scan step / observation | must not be sole identity key |

**Parse-scoped honesty:** [`PlaylistSidebarScan`](../../crates/auv-netease-music/src/lib.rs)
L317–325 states all reachable `id` fields are unique within one scan only until
ViewMemory + content-derived IDs land.

## Evidence files

All curated files live under:

```text
docs/ai/references/evidence/2026-06-30-scenebridge-netease-sidebar/
```

| File | Meaning |
| --- | --- |
| [`README.md`](evidence/2026-06-30-scenebridge-netease-sidebar/README.md) | Folder index |
| [`hermetic-reconstruct-sidebar-synthetic.json`](evidence/2026-06-30-scenebridge-netease-sidebar/hermetic-reconstruct-sidebar-synthetic.json) | Redacted projection + match sample from sidebar test vectors |
| [`match-ref-vocabulary.json`](evidence/2026-06-30-scenebridge-netease-sidebar/match-ref-vocabulary.json) | Example `MatchRef` fields + glossary |
| [`gap-view-memory-and-reacquire.txt`](evidence/2026-06-30-scenebridge-netease-sidebar/gap-view-memory-and-reacquire.txt) | ViewMemory unimplemented + playlist select rescan NOTICE |

## Hermetic positive evidence

### Source test

[`reconstruct_sidebar_groups_items_under_carried_section`](../../crates/auv-netease-music/src/view_parsers/sidebar/tests.rs)
L7–54:

- **Input:** two fake viewport pages (page0: 创建的歌单 + Coding BGM; page1: Jazz +
  收藏的歌单 + Road Trip).
- **Output:** 2 projection sections, 3 items total.
- **Assertions:** section carry (`section_hint`), reconstruction root
  `Collection` with 2 children.

Synthetic JSON in the evidence folder redacts internal observation geometry and
uses illustrative IDs — it is **authored for documentation**, not a `cargo test`
capture.

### Merge policy pointer

Cross-viewport merge behavior (carry, dedup, section landmarks) is specified in
[view-parser-merge-fixtures-v0](2026-05-29-view-parser-merge-fixtures-v0.md).
A2 does not duplicate the full fixture corpus; refer to that doc for case names.

## Negative / gap evidence

| Gap | Evidence |
| --- | --- |
| ViewMemory not implemented | [view-memory-v0](2026-05-29-view-parser-view-memory-v0.md) spec; zero `auv-view::memory` Rust |
| `playlist select` rescan instead of reacquire | [`playlist.rs` NOTICE](../../crates/auv-netease-music/src/commands/playlist.rs) L353–357 |
| Promotion `StabilityUnproven` default | [`candidate_promotion.rs`](../../src/candidate_promotion.rs) L153–156 |
| Spec vs shipped gap | [spec-vs-pr9 triage §10](2026-05-29-view-parser-spec-vs-pr9-divergence-triage.md) |

## What this pack proves

- NetEase sidebar view-parser can **parse → reconstruct → project** playlist
  sections and items from hermetic OCR-like vectors.
- Merge/carry produces stable **section** and **item** labels across two viewport
  pages in the canonical test fixture.
- Agent-facing **`MatchRef`** exposes `section_id`, `item_id`, `anchor_id`,
  `candidate_id`, and `label` for `playlist` CLI JSON output.
- **CLI binding today:** `playlist play --candidate-id` (id path); `playlist select <label>` (keyword path).
- **Gaps are explicit:** ViewMemory, reacquire, and core `CandidateRef` promotion
  are not landed; `playlist select` replays scan pages.

## What this pack does not prove

- Live NetEase desktop behavior, screenshots, or playback verification
- Cross-app scene identity (QQ Music, other donors)
- Session API or MCP invoke binding for sidebar commands
- Root catalog `command_id` for NetEase operations
- Durable cross-run `anchor_id` without ViewMemory (IDs are parse-scoped today)
- `candidate-action` or archived AX copilot promotion paths

## Debt → A3 trigger

Signing A2 Package A unlocks **candidate** A3 work (not auto-implement):

| Debt | A3 candidate |
| --- | --- |
| ViewMemory writer/reader | `auv-view::memory` minimal impl |
| Reacquire vs rescan | Replace `NOTICE(netease-playlist-select-reacquire)` path |
| Hermetic regression | Tests using curated A2 fixtures |
| Live proof (optional) | One `live`-labeled desktop run if owner requests |
| Catalog binding | Only if owner names cross-frontend consumer (Package B fork) |

## Validation

```sh
git diff --check
rg -n "SceneBridge|ViewMemory|MatchRef" docs/ai/references/2026-06-30-auv-scenebridge-a2-*
cargo test -p auv-netease-music view_parsers::sidebar  # optional cross-check
```

Expected: sidebar hermetic tests pass; evidence JSON is synthetic; no new Rust in
this slice.

## Related

- [A2 boundary decision review](2026-06-30-auv-scenebridge-a2-boundary-decision-review.md)
- [A1 design charter](2026-06-30-auv-scenebridge-a1-design-charter.md)
- [view-memory-v0](2026-05-29-view-parser-view-memory-v0.md)
- [contract-bridge-v0](2026-05-29-view-parser-contract-bridge-v0.md)
- [P14 API pause](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md)
