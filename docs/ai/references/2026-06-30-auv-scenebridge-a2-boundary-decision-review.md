# SceneBridge A2: NetEase Sidebar Boundary Decision Review

**Date:** 2026-06-30  
**Status:** **owner accepted Package A** — final decision record for SceneBridge A2.
A3 prototype boundary landed separately. Session API P14 pause unchanged.

**Prior work:** [A1 design charter](2026-06-30-auv-scenebridge-a1-design-charter.md)
(docs-only lane framing) → A2 evidence pack + boundary forks (this note).

## One-line summary

NetEase Music **playlist sidebar** is the first SceneBridge donor surface. This
review locks identity descriptor, implementation locus, promotion path, command
binding, and proof boundary **before** any A3 `ViewMemory` / reacquire prototype.
**Owner answer: Package A** — no new `contract.rs` scene type, `auv-view::memory`
only, surface-analyze single gate, product CLI binding first, hermetic evidence
only.

## Owner freeze block

```text
scene identity descriptor：ViewAnchor + ViewMemory scope（no new contract.rs SceneTarget in A2）
implementation locus：auv-view::memory（no auv-scenebridge crate in A2）
promotion：surface-analyze single gate only
command binding：auv-netease-music CLI first（no root catalog in A2）
proof：hermetic curated evidence only（live → A3）
A3-impl：frozen unless owner reopens with Package B + named consumer
```

### English expansion (for reviewers)

| Statement | Meaning | Evidence |
| --- | --- | --- |
| Scene identity = ViewAnchor + ViewMemory scope | Cross-run scene target keys use `app_bundle_id`, `scope_id` / `projection_id`, and `anchor_id` from view-parser IR — not a new `SceneTarget` in `contract.rs` | [view-memory-v0](2026-05-29-view-parser-view-memory-v0.md), [`MatchRef`](../../crates/auv-netease-music/src/output.rs), [`PlaylistSidebarScan` doc](../../crates/auv-netease-music/src/lib.rs) L317–325 |
| Implementation locus `auv-view::memory` | `ViewMemory` writer/reader lives in existing `auv-view` crate per placement spec | [view-memory-v0](2026-05-29-view-parser-view-memory-v0.md) L60–62; **zero Rust** in `auv-view::memory` today |
| Surface-analyze single gate | View-derived actionable targets promote only through surface-analyze → `contract::Candidate` | [contract-bridge-v0](2026-05-29-view-parser-contract-bridge-v0.md) L70–75 |
| Product CLI binding first | `playlist ls --json` exposes `MatchRef`; `playlist play --candidate-id` consumes `candidate_id`; `playlist select <label>` uses keyword query — root [`catalog.rs`](../../src/catalog.rs) unchanged | [`cli.rs`](../../crates/auv-netease-music/src/cli.rs), [`playlist.rs` NOTICE](../../crates/auv-netease-music/src/commands/playlist.rs) L353–357 |
| Hermetic proof only in A2 | Evidence pack is curated spec + synthetic fixtures; live desktop proof deferred to A3 | [A2 evidence pack](2026-06-30-auv-scenebridge-a2-netease-sidebar-evidence-pack.md) |
| A3 frozen | No `ViewMemory` impl, reacquire, or catalog promotion without owner reopen | This note §Reopen triggers |

## Slice classification

| Item | Value |
| --- | --- |
| This note (SceneBridge A2 boundary) | **docs-only** |
| Follow-on code (SceneBridge A3, if approved) | **owner-approved feature** |
| Not | bug fix, test-only, narrow refactor |

## Problem / why now

A1 framed cross-app scene identity as a design lane but deferred architecture
forks. A1 reopen trigger: *owner names SceneBridge A2 evidence pack → grounding
vocabulary + fixture scope*.

The richest NetEase grounding path already exists in
[`auv-netease-music` sidebar view-parser](../../crates/auv-netease-music/src/view_parsers/sidebar/)
with hermetic tests and agent-facing [`MatchRef`](../../crates/auv-netease-music/src/output.rs).
Without an explicit boundary decision, A3 work risks:

- inventing `SceneTarget` / `SceneIdentity` in `contract.rs` parallel to view IR,
- minting a new `auv-scenebridge` crate duplicating `auv-view::memory`,
- bypassing the surface-analyze promotion gate,
- promoting NetEase commands into root catalog without a named cross-frontend consumer,
- over-claiming live desktop behavior in documentation.

## Evidence / inventory

### NetEase playlist sidebar (richest donor)

| Surface | Location | Role |
| --- | --- | --- |
| Parse → reconstruct → projection | [`view_parsers/sidebar/`](../../crates/auv-netease-music/src/view_parsers/sidebar/) | Two-viewport merge, section carry, dedup |
| Hermetic positive fixture | [`tests.rs` `reconstruct_sidebar_groups_items_under_carried_section`](../../crates/auv-netease-music/src/view_parsers/sidebar/tests.rs) L7–54 | 2 pages → 2 sections, 3 items |
| Agent-facing identity | [`output.rs` `MatchRef`](../../crates/auv-netease-music/src/output.rs) L8–15 | `section_id`, `item_id`, `anchor_id`, `candidate_id`, `label` |
| Parse-scoped ID honesty | [`lib.rs` `PlaylistSidebarScan`](../../crates/auv-netease-music/src/lib.rs) L317–325 | IDs unique within one scan; cross-run durability needs ViewMemory |
| Selection without reacquire | [`playlist.rs` NOTICE](../../crates/auv-netease-music/src/commands/playlist.rs) L353–357 | Full rescan replay instead of view-memory reacquire |

### ViewMemory gap (spec vs code)

| Item | Status |
| --- | --- |
| [view-memory-v0](2026-05-29-view-parser-view-memory-v0.md) | Full persistence shape, keyed by `app_bundle_id` + `scope_id` |
| `auv-view::memory` Rust | **Not implemented** |
| [spec-vs-pr9 triage §10](2026-05-29-view-parser-spec-vs-pr9-divergence-triage.md) | ViewMemory + reacquire spec'd; PR-era single-shot scan only |

### Promotion and contract discipline

| Rule | Source |
| --- | --- |
| One `ArtifactRef` schema | [contract-bridge-v0](2026-05-29-view-parser-contract-bridge-v0.md) |
| Single promotion gate to `contract::Candidate` | surface-analyze v0 via contract-bridge |
| `StabilityUnproven` default refusal | [`candidate_promotion.rs`](../../src/candidate_promotion.rs) L153–156 |
| `MatchRef` ≠ `CandidateRef` | Product CLI output vs core operation-scoped candidate |

### Command binding today

| Surface | Bound? |
| --- | --- |
| `auv-netease-music playlist` / `playlist play --candidate-id` | **yes** — product crate CLI (`playlist select <label>` is label query, not id-based) |
| Root `src/catalog.rs` `command_id` | **no** — deferred until owner names cross-frontend consumer |
| Session API / MCP invoke | **separate lane** — [P14 pause](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md) |

### Flow (read-side grounding → action)

```text
Sidebar OCR / AX observations
  → parse_sidebar_viewport (per page)
  → reconstruct_playlist_sidebar
  → PlaylistSidebarProjection + ViewReconstruction (anchors)
  → MatchRef (agent JSON today)
  → (gap) ViewMemory persist + reacquire
  → (future) AppSurfaceCandidate → surface-analyze → contract::Candidate
  → playlist play --candidate-id / playlist select <label> (product CLI first)
```

## Options analysis — D1 through D5

### D1 Scene descriptor — new `SceneTarget` in `contract.rs`?

| Package A (recommend) | Package B (defer) |
| --- | --- |
| **No new type in A2/A3-first slice.** Scene identity = `ViewAnchor` + ViewMemory scope (`app_bundle_id`, `scope_id`, `anchor_id`) bridged via `MatchRef` / future `CandidateRef` | Add `SceneTarget` / `SceneIdentity` to `contract.rs` before ViewMemory lands |

**Reviewer recommendation:** Package A. View-parser IR already carries anchors;
`PlaylistSidebarScan` documents parse-scoped limits honestly. A dedicated
`contract.rs` type duplicates view IR without a proven cross-app consumer.

### D2 Implementation locus — new crate vs extend `auv-view`?

| Package A (recommend) | Package B (defer) |
| --- | --- |
| **`auv-view::memory` only** per view-memory-v0 placement | New `auv-scenebridge` crate |

**Reviewer recommendation:** Package A. [view-memory-v0](2026-05-29-view-parser-view-memory-v0.md)
pins placement; a parallel crate splits ownership without a second donor app in A2.

### D3 Promotion path — how grounding becomes actionable?

| Package A (recommend) | Package B (defer) |
| --- | --- |
| **Single gate:** ViewMemory/reacquire → `AppSurfaceCandidate` → surface-analyze → `contract::Candidate` → `CandidateRef` | Scene-specific promotion shortcut |

**Reviewer recommendation:** Package A. [contract-bridge-v0](2026-05-29-view-parser-contract-bridge-v0.md)
forbids parallel scene gates and direct `contract::Candidate` minting from view-parser.

### D4 Command binding — how scene binds to invoke surface?

| Package A (recommend) | Package B (defer) |
| --- | --- |
| **Product CLI binding first:** `playlist ls --json` + `playlist play --candidate-id`; `playlist select <label>` for keyword path; root catalog deferred | Promote NetEase commands into root catalog in A3 without named consumer |

**Reviewer recommendation:** Package A. NetEase commands live in product crate;
root catalog changes need a named MCP/session/CLI consumer per invoke discipline.

### D5 Proof boundary — what A2 evidence may claim?

| Package A (recommend) | Package B (defer) |
| --- | --- |
| **Hermetic + curated spec/fixture only**; live desktop proof labeled `live` and deferred to A3 | Include new live transcripts in A2 pack |

**Reviewer recommendation:** Package A. No committed NetEase screenshot/`.auv`
artifacts in repo; A2 curates from tests and specs only.

## Owner decision packages

Answer **before** any SceneBridge A3 work.

### Package A — ViewAnchor + ViewMemory + product CLI (**accepted**)

**Owner: Package A accepted** — 2026-06-30, matches reviewer recommendation.

```text
A2-A  No SceneTarget in contract.rs for A2/A3-first slice
A2-B  ViewMemory in auv-view::memory only
A2-C  surface-analyze single promotion gate
A2-D  auv-netease-music CLI binding; no root catalog in A2
A2-E  Hermetic curated evidence only; live → A3
```

**When to choose:** NetEase sidebar is first donor; view-parser specs and hermetic
tests are sufficient to lock vocabulary and gaps; session API and catalog parity
stay out of scope.

### Package B — Contract type + scene crate + catalog — **not accepted**

```text
A2-A  Add SceneTarget / SceneIdentity to contract.rs before memory lands
A2-B  New auv-scenebridge crate
A2-C  Scene-specific promotion shortcut
A2-D  Root catalog command_id for NetEase in A3 without named consumer
A2-E  Live desktop evidence in A2 pack
```

**When to choose:** Owner names a cross-app `contract.rs` consumer that cannot
reuse view IR, or requires a dedicated scene crate before a second donor app exists.

## Anti-misread rules (frozen)

1. **SceneBridge ≠ session API** — grounding lane is read-side; [P14](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md)
   pause is unchanged. Do not route scene identity through session RPC.

2. **A2 ≠ ViewMemory implementation** — A2 is docs + curated evidence only.
   `auv-view::memory` Rust is A3 candidate work.

3. **`MatchRef` ≠ `CandidateRef`** — `MatchRef` is product CLI agent JSON;
   `CandidateRef` is operation-scoped core contract after promotion gate.

4. **`MatchRef` IDs are parse-scoped today** — [`PlaylistSidebarScan`](../../crates/auv-netease-music/src/lib.rs)
   documents that `anchor_id` / `item_id` are not durable across runs until
   ViewMemory + content-derived identity land.

5. **Single promotion gate** — view-parser must not mint `contract::Candidate`
   directly; see contract-bridge-v0.

6. **A2 boundary review ≠ A3 auto-start** — owner must name A3 explicitly.

7. **A2 does not unlock P14, R2b-impl, or MCP merge** — independent lanes.

8. **QQ Music evidence is a different path** — cross-app compare is A3+ optional;
   do not copy QQ OCR packs into NetEase A2 folder.

9. **Hermetic evidence ≠ live proof** — synthetic JSON in evidence folder is
   authored from test vectors; label any future desktop run `live`.

10. **`playlist select` ≠ id-based binding** — `playlist select <label>` is keyword
    query; durable id path is `playlist play --candidate-id` after `playlist ls --json`.

11. **Signing A2 Package A does not add TERMS entries** — provisional vocabulary
    lives in A2 evidence pack; full `TERMS_AND_CONCEPTS.md` update deferred to A3
    unless owner names a TERMS slice.

## Explicit non-goals (SceneBridge A2)

This note does **not** approve:

- Rust, proto, or transport code changes
- `ViewMemory` writer/reader implementation (A3)
- Session API / P14 reopen or subprocess smoke expansion
- `candidate-action` archived vertical expansion
- Root [`catalog.rs`](../../src/catalog.rs) `command_id` entries for NetEase
- New `auv-scenebridge` crate
- AIRI orchestration shell import into AUV core
- QQ Music path or cross-app compare in A2
- New live screenshot / `.auv` artifact capture runs for this pack

## Reopen triggers for A3

| Trigger | Unlocks | Does **not** auto-unlock |
| --- | --- | --- |
| Owner names **SceneBridge A3** prototype | Minimal `auv-view::memory` + NetEase `playlist select` reacquire replacing rescan NOTICE | **Landed 2026-06-30** — [A3 boundary](2026-06-30-auv-scenebridge-a3-prototype-boundary-review.md) |
| Owner signs **Package B** + named cross-frontend consumer | Revisit D1/D4 (contract type, catalog binding) | A3 without explicit owner name |
| Owner names **TERMS** slice for scene identity | Provisional → durable vocabulary in `TERMS_AND_CONCEPTS.md` | ViewMemory impl by itself |

### A3 prototype (landed 2026-06-30)

Owner named SceneBridge A3 after A2 Package A. See:

- [A3 prototype boundary review](2026-06-30-auv-scenebridge-a3-prototype-boundary-review.md) — **Package A3-min accepted**
- [A3 implementation handoff](2026-06-30-auv-scenebridge-a3-implementation-handoff.md)

## Open questions — resolved (Package A)

| # | Question (A1) | Package A resolution |
| --- | --- | --- |
| 1 | First evidence pack app / surface? | **NetEase Music playlist sidebar** — [A2 evidence pack](2026-06-30-auv-scenebridge-a2-netease-sidebar-evidence-pack.md) |
| 2 | Prototype crate vs `auv-view` extension? | **`auv-view::memory` only** — no `auv-scenebridge` crate |
| 3 | Live vs hermetic proof boundary? | **Hermetic curated only in A2**; live deferred to A3 |

## Relationship to A1 / view-parser / API pause

- **A1** framed the lane; A2 closes architecture forks for the first donor.
- **view-memory-v0** + **contract-bridge-v0** are normative for A3 shape.
- **P14** session API pause unchanged; SceneBridge does not consume session rhythm.
- **AIRI** may donate driver primitives only — [reuse note](2026-05-13-auv-airi-desktop-reuse.md).

## Validation (readers re-checking evidence)

```sh
git diff --check
rg -n "SceneBridge|ViewMemory|MatchRef" docs/ai/references/2026-06-30-auv-scenebridge-a2-*
cargo test -p auv-netease-music view_parsers::sidebar  # optional cross-check
```

Expected: boundary + evidence-pack docs cite D1–D5 and Package A; sidebar hermetic
tests pass; `auv-view::memory` implementation follows [A3 handoff](2026-06-30-auv-scenebridge-a3-implementation-handoff.md).

## Related

- [A1 design charter](2026-06-30-auv-scenebridge-a1-design-charter.md)
- [A3 prototype boundary review](2026-06-30-auv-scenebridge-a3-prototype-boundary-review.md)
- [A3 implementation handoff](2026-06-30-auv-scenebridge-a3-implementation-handoff.md)
- [A2 NetEase sidebar evidence pack](2026-06-30-auv-scenebridge-a2-netease-sidebar-evidence-pack.md)
- [view-memory-v0](2026-05-29-view-parser-view-memory-v0.md)
- [contract-bridge-v0](2026-05-29-view-parser-contract-bridge-v0.md)
- [view-parser merge fixtures v0](2026-05-29-view-parser-merge-fixtures-v0.md)
- [P14 API line pause](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md)
- [AIRI desktop reuse](2026-05-13-auv-airi-desktop-reuse.md)
