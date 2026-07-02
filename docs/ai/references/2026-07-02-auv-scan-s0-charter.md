# AUV Scan S0: 2D Temporal Scan Design Charter

**Date:** 2026-07-02  
**Status:** design charter — docs-only; no implementation  
**Server API needed:** **No** (S0)  
**Slice:** independent S line (note-level INDEX entry; not a separate lane taxonomy yet)

## One-line summary

S0 frames the **scan line**: from a **single viewport, continuous frame sequence**, produce
**time-continuous, auditable 2D UI observation** — what is visible, what persists across
frames, how the viewport moved, why targets were lost or reacquired, and which conclusions
are trustworthy.

**First goal is “see and follow” in 2D — not 3D scene modeling.**

## Primary user problem

Operators and automation need to reason about a GUI surface **over time**, not only on
one static capture. Scroll, animation, and partial occlusion make per-frame OCR or detection
insufficient without temporal association and honest diagnostics.

### Five auditable questions (fixed)

Every S-line output must eventually help answer:

- 当前视口里有什么？
- 哪些对象是同一个东西（跨帧身份）？
- 视口如何移动了（滚动/平移/裁剪）？
- 为什么某个目标丢了 / 找回了？
- 哪些结论可信，哪些只是弱证据？

## Relationship to existing assets

| Asset | Path | S-line stance |
|-------|------|---------------|
| Scroll scan design | [2026-05-21-scroll-scan-design.md](2026-05-21-scroll-scan-design.md) | **Complementary donor** — observe → scroll → merge **page-level** evidence with stop policy; not cross-frame track association |
| Scroll scan runtime | [`src/scroll_scan/mod.rs`](../../../src/scroll_scan/mod.rs) `ScrollScanArtifact` | Page/snapshot shapes may inform artifacts; **no S0 code change** |
| View evidence | [`ViewEvidenceNode`](../../../crates/auv-view/src/lib.rs) | Evidence provenance patterns; reuse vs extend → S1 implementation slice |
| SceneBridge | [A1 charter](2026-06-30-auv-scenebridge-a1-design-charter.md), B1–B2c handoffs | **S3** — memory / reacquire / inspect consumption; do not mix rhythm in S0/S1 |
| Core lane roadmap | [2026-06-13-auv-core-lane-roadmap.md](2026-06-13-auv-core-lane-roadmap.md) | S line follows core invoke + run recording model |

```text
continuous frames (single viewport)
  → temporal 2D observation (S1)
  → optional app projection (S2)
  → memory / inspect wiring (S3)
```

SceneBridge identity/grounding consumes stable observations **later**; S0 does not redefine
`CandidateRef` or invoke semantics.

## Input (conceptual)

| Input | Required | Notes |
|-------|----------|-------|
| Frame sequence | yes | Screenshots or video frames from one window / one viewpoint |
| Timestamps | yes | Per-frame ordering and delta reasoning |
| Window / crop bounds | optional | Normalizes coordinates across frames |
| App hint / query / custom words | optional | Not semantic closure in S0 — query-aware scan → S2 |
| Raw OCR results | optional | Per-frame donor evidence |
| Raw detector results | optional | **Optional donor** — not required for S0 charter |

## Output (conceptual — provisional vocabulary)

`TemporalScanResult` (provisional name only; not an approved wire contract) should carry:

| Field | Purpose |
|-------|---------|
| `frame_observations[]` | Per-frame visible evidence summary |
| `viewport_timeline[]` | Estimated scroll / pan / crop motion over time |
| `tracks[]` | Cross-frame stable object associations |
| `confidence` | Frame-level, track-level, and result-level scores |
| `diagnostics[]` | Honest miss / unstable / ambiguous explanations |

### Artifact naming (convention only — not implemented in S0)

- `scan-frame-0001.json`, `scan-frame-0002.json`, …
- `scan-timeline.json`
- `scan-tracks.json`
- Optional: `scan-frame-0001.png` or crops

Artifacts should be inspectable and replayable under implicit run recording, same spirit as
existing scroll-scan artifacts.

## Explicit non-goals (S0 / whole S line until reopened)

- 3DGS / NeRF / monocular 3D scene reconstruction
- General SLAM / world-coordinate mapping
- Multi-camera / multi-view fusion
- Action execution / agent policy
- Cross-session global knowledge base
- App-specific query semantic closure (→ **S2**)
- New inspect server compare APIs (see [B2c deferred](2026-06-30-auv-scenebridge-b2c-inspect-cross-run-compare-deferred.md))
- Session API / invoke runtime changes

## Success criteria (measurable — not “looks smart”)

| Metric | Intent |
|--------|--------|
| **track continuity** | Same visible object keeps stable identity across frames when evidence supports it |
| **ID switch rate** | Count unjustified track id changes (lower is better) |
| **viewport motion accuracy** | Estimated scroll/pan matches fixture ground truth or corroborating diff |
| **reacquire honesty** | Reappearance links to prior track or reports ambiguity — no silent new hallucinated objects |
| **diagnostics completeness** | Every weak or failed conclusion has a diagnostic code, not empty output |

## Validation posture

| Layer | Role |
|-------|------|
| **Hermetic** | Recorded frame fixture sequences; no live app, network, or desktop state |
| **Live** | High-scroll, high-duplicate-text probe scenario — protocol described in S1 plan; not implemented in S0 |

Hermetic tests are the default regression gate. Live probes are labeled explicitly when used.

## Phase map

| Phase | Content | Status |
|-------|---------|--------|
| **S0** | This charter — problem, boundaries, validation posture | **current** |
| **S1** | [2D temporal scan core plan](2026-07-02-auv-scan-s1-temporal-core-plan.md) | planned; blocked on S0 merge |
| **S2** | App-specific projection / query-aware scan | direction only |
| **S3** | ViewMemory / reacquire / inspect consumption wiring | direction only |
| **S4+** | 3D / scene-model **research lane** — only if S1–S3 prove single-viewport geometric recovery is worth it | **not roadmap** |

**Principle:** 先把“看见并跟住”做对，再谈建模。

## Reopen / next slice trigger

| Trigger | Unlocks |
|---------|---------|
| Owner names **S1 implementation** after S0 charter is merged | Execute [S1 plan](2026-07-02-auv-scan-s1-temporal-core-plan.md) slices — still requires per-slice owner approval |
| ≥2 Evidence records per [B2c-style gate](2026-06-30-auv-scenebridge-b2c-inspect-cross-run-compare-deferred.md) pattern | N/A for S0; listed so S3 does not reopen compare early |

Signing S0 does **not** approve Rust types, crates, or server APIs.

## TODO(terms)

When S1 implementation locks contract names and artifact roles, update
[`docs/TERMS_AND_CONCEPTS.md`](../../TERMS_AND_CONCEPTS.md) — do not define durable terms only in
this charter.

## Related

- [S1 temporal core plan](2026-07-02-auv-scan-s1-temporal-core-plan.md)
- [Scroll scan design](2026-05-21-scroll-scan-design.md)
- [SceneBridge A1 charter](2026-06-30-auv-scenebridge-a1-design-charter.md)
- [Core lane roadmap](2026-06-13-auv-core-lane-roadmap.md)

## Validation (this document only)

```sh
git diff --check
```
