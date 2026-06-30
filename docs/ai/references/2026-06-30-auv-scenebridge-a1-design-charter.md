# SceneBridge A1: Cross-App Scene Identity Design Charter

**Date:** 2026-06-30  
**Status:** design charter only — no implementation approved  
**Slice:** docs-only design (independent lane)

**AIRI boundary:** [2026-05-13-auv-airi-desktop-reuse.md](2026-05-13-auv-airi-desktop-reuse.md)  
**Session API lane:** [API-P14 pause](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md) — **separate rhythm**

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
  → stable scene target descriptor (A2+ TBD)
  → catalog command_id / typed operation binding
  → existing invoke + run recording + inspect path
```

SceneBridge sits on the **read / grounding** side. It must not fork parallel
`OperationResult` or session RPC semantics.

Relevant contract types today: `CandidateRef`, `RecognitionResult`,
`VerificationResult` in [`contract.rs`](../../src/contract.rs). A2+ must say
whether SceneBridge produces, narrows, or reuses these — not invent duplicates.

## In-scope questions for A2+

1. **Identity keys** — what is stable across refocus and layout drift?
2. **Evidence sources** — AX tree, vision, driver geometry; merge policy
3. **Promotion boundary** — when does grounding become a `CandidateRef`-class target?
4. **Inspect contract** — what artifacts/trace events prove identity decisions?
5. **Cross-app scope** — same command class across apps vs per-app scene namespaces

## Explicit non-goals (A1)

- Session API unary work ([P14](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md) pause)
- [API-P13](2026-06-30-auv-api-p13-external-client-smoke-handoff.md) / [API-S1](2026-06-30-auv-api-s1-subprocess-smoke-handoff.md) smoke
- AIRI server shell, MCP tool descriptors, approval queues
- `candidate-action` archived vertical expansion
- Rust, proto, or new crates in A1
- Mixing SceneBridge implementation rhythm with session API slices

## AIRI separation

AIRI may donate **driver-layer primitives** (screenshot, input, capability habits)
per the reuse note. SceneBridge design and AIRI product guidance stay in **separate
checkouts** unless an owner names an explicit cross-scope slice.

Do not import AIRI orchestration shells into AUV core as part of this lane.

## Open questions (blocking A2)

| # | Question | A1 stance |
| --- | --- | --- |
| 1 | First evidence pack app / surface? | Owner-named — not chosen in A1 |
| 2 | Prototype crate vs `auv-view` extension? | Deferred to A2 architecture slice |
| 3 | Live vs hermetic proof boundary? | Must be explicit in A2; no desktop-coupled unit tests by default |

## Reopen triggers (candidate only)

| Trigger | Unlocks |
| --- | --- |
| Owner names **SceneBridge A2** evidence pack | Grounding vocabulary + fixture scope |
| Owner names **SceneBridge A3** prototype | Narrow read-side producer behind feature gate |

Signing A1 does **not** unlock session API P10, R2b-impl, or MCP merge.

## Related

- [TERMS_AND_CONCEPTS.md](../../TERMS_AND_CONCEPTS.md)
- [API-L1 operator guide](2026-06-30-auv-api-l1-session-api-operator-guide.md) (session lane — separate)
- [P4 server seam](2026-06-30-auv-api-p4-session-proto-server-seam-design.md)
