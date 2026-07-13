# SceneBridge A1: Cross-App Scene Identity Design Charter

**Date:** 2026-06-30  
**Status:** design charter — A2 boundary + evidence landed 2026-06-30; A3 prototype boundary landed 2026-06-30; Rust per A3 handoff
**Slice:** docs-only design (independent lane)

**AIRI boundary:** [2026-05-13-auv-airi-desktop-reuse.md](../ops/2026-05-13-airi-desktop-reuse.md)  
**Session API lane:** [API-P14 pause](../session-api/2026-06-30-session-api-closeout.md) — **separate rhythm**

## One-line summary

SceneBridge A1 frames a future lane: **cross-app scene identity / grounding that
binds stable targets to AUV catalog `command_id` and typed operation entrypoints**,
without replacing invoke, run recording, or the paused session API unary surface.

## Problem statement (owner-confirmed)

Operators and automation need a target that survives:

- app switches and refocus,
- window geometry changes,
- surface re-layout within an app,

and still resolves to a **catalog command** or typed operation the runtime can
invoke with inspectable evidence.

Today AUV has recognition → candidate → action seams
(`RecognitionResult` → `CandidateRef` → verification) in core contracts, but no
dedicated **cross-app scene identity** lane that explains how grounding evidence
feeds those types consistently across apps.

SceneBridge is that design lane — **not** a second invoke runtime.

## Relationship to existing seams

```text
Scene identity evidence (AX / vision / driver)
  → ViewAnchor + ViewMemory scope (A2 Package A)
  → product CLI / catalog command binding
  → existing invoke + run recording + inspect path
```

SceneBridge sits on the **read / grounding** side. It must not fork parallel
`OperationResult` or session RPC semantics.

Relevant contract types today: `CandidateRef`, `RecognitionResult`,
`VerificationResult` in [`contract.rs`](../../../../src/contract.rs). A2+ must say
whether SceneBridge produces, narrows, or reuses these — not invent duplicates.

## In-scope questions for A2+

1. **Identity keys** — what is stable across refocus and layout drift?
2. **Evidence sources** — AX tree, vision, driver geometry; merge policy
3. **Promotion boundary** — when does grounding become a `CandidateRef`-class target?
4. **Inspect contract** — what artifacts/trace events prove identity decisions?
5. **Cross-app scope** — same command class across apps vs per-app scene namespaces

## Explicit non-goals (A1)

- Session API unary work ([P14](../session-api/2026-06-30-session-api-closeout.md) pause)
- [API-P13](../session-api/2026-06-30-session-api-closeout.md) / [API-S1](../session-api/2026-06-30-session-api-closeout.md) smoke
- AIRI server shell, MCP tool descriptors, approval queues
- `candidate-action` archived vertical expansion
- Rust, proto, or new crates in A1
- Mixing SceneBridge implementation rhythm with session API slices

## AIRI separation

AIRI may donate **driver-layer primitives** (screenshot, input, capability habits)
per the reuse note. SceneBridge design and AIRI product guidance stay in **separate
checkouts** unless an owner names an explicit cross-scope slice.

Do not import AIRI orchestration shells into AUV core as part of this lane.

## Open questions — resolved by A2 (2026-06-30)

| # | Question | A2 resolution (Package A) |
| --- | --- | --- |
| 1 | First evidence pack app / surface? | **NetEase Music playlist sidebar** — [A2 evidence pack](2026-06-30-scenebridge-netease-sidebar-evidence-pack.md) |
| 2 | Prototype crate vs `auv-view` extension? | **`auv-view::memory` only** — [A2 boundary review](2026-06-30-scenebridge-boundary-decision-review.md) D2 |
| 3 | Live vs hermetic proof boundary? | **Hermetic curated only in A2**; live optional in A3 — boundary review D5 |

## Open questions — partial A3 resolution (2026-06-30)

| # | Question | A3 resolution (Package A3-min) |
| --- | --- | --- |
| 4 | Inspect contract — artifacts/trace for identity? | **Partial → A5 freeze:** [A5 inspect identity proof charter](2026-06-30-scenebridge-inspect-identity-proof-charter.md); trace/inspect API impl → future slice |
| 5 | Cross-app scope? | **Deferred** — NetEase `playlist_sidebar` only |

## Reopen triggers (candidate only)

| Trigger | Unlocks | Status |
| --- | --- | --- |
| Owner names **SceneBridge A2** evidence pack | Grounding vocabulary + fixture scope | **Landed 2026-06-30** — [boundary review](2026-06-30-scenebridge-boundary-decision-review.md), [evidence pack](2026-06-30-scenebridge-netease-sidebar-evidence-pack.md) |
| Owner names **SceneBridge A3** prototype | Narrow read-side producer behind feature gate | **Landed 2026-06-30** — [A3 boundary](2026-06-30-scenebridge-prototype-boundary-review.md), [A3 handoff](2026-06-30-scenebridge-closure.md) |

Signing A1 does **not** unlock session API P10, R2b-impl, or MCP merge.

## Related

- [A3 prototype boundary review](2026-06-30-scenebridge-prototype-boundary-review.md) — **Owner: Package A3-min accepted**
- [A3 implementation handoff](2026-06-30-scenebridge-closure.md)
- [A2 NetEase sidebar evidence pack](2026-06-30-scenebridge-netease-sidebar-evidence-pack.md)
- [view-memory-v0](../view-memory/2026-05-29-view-parser-view-memory-v0.md)
- [contract-bridge-v0](../view-memory/2026-05-29-view-parser-contract-bridge-v0.md)
- [TERMS_AND_CONCEPTS.md](../../../TERMS_AND_CONCEPTS.md)
- [API-L1 operator guide](../session-api/2026-06-30-api-session-api-operator-guide.md) (session lane — separate)
- [P4 server seam](../session-api/2026-06-30-api-session-proto-server-seam-design.md)
