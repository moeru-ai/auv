# 2026 07 03 Scan Temporal Core Landed

Durable reference folded from intermediate handoffs along the same responsibility line. Historical slice codes are omitted from navigation; see absorbed sources below.

## Status

Merged closeout / landed reference. Prefer this document over the absorbed intermediate notes.

## Absorbed sources

- **AUV Scan S1: 2D Temporal Scan Core — Implementation Plan** — formerly `2026-07-02-auv-scan-s1-temporal-core-plan.md`
- **AUV Scan S1：Slice 2–4 工程实施计划（Producer → Read → Temporal）** — formerly `2026-07-02-auv-scan-s1-s2-s4-producer-read-temporal-plan.md`
- **AUV Scan S1 Slices 2–4 — GAN Implementation Spec** — formerly `2026-07-02-auv-scan-s1-s2-s4-gan-spec.md`
- **AUV Scan S1 Slice 1: Frame + Artifact Contract — Implementation Handoff** — formerly `2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md`
- **AUV Scan S1 Slice 2: Producer Wiring — Implementation Handoff** — formerly `2026-07-02-auv-scan-s1-slice2-producer-handoff.md`
- **AUV Scan S1 Slice 3: Read-side Consume — Implementation Handoff** — formerly `2026-07-02-auv-scan-s1-slice3-read-side-handoff.md`
- **AUV Scan S1-4a: Multi-frame Artifacts — Implementation Handoff** — formerly `2026-07-02-auv-scan-s1-s4a-multi-frame-handoff.md`
- **AUV Scan S1-4b: Two-frame Motion / Timeline — Implementation Handoff** — formerly `2026-07-03-auv-scan-s1-s4b-motion-timeline-handoff.md`
- **AUV Scan S4 Lifecycle Evaluator v1 — Handoff** — formerly `2026-07-02-auv-scan-s4-lifecycle-evaluator-handoff.md`
- **AUV Scan S7: Invoke Frame Producer — Implementation Handoff** — formerly `2026-07-06-auv-scan-s7-invoke-frame-producer-handoff.md`

## Folded notes

### AUV Scan S1: 2D Temporal Scan Core — Implementation Plan

_Source: `2026-07-02-auv-scan-s1-temporal-core-plan.md`_

**Date:** 2026-07-02 **Status:** implementation plan — **not started** **Prerequisite:** [S0 charter](2026-07-02-scan-charter.md) **Server API needed:** **No** (S1 v0 — client-side artifacts + implicit run recording only)

### AUV Scan S1：Slice 2–4 工程实施计划（Producer → Read → Temporal）

_Source: `2026-07-02-auv-scan-s1-s2-s4-producer-read-temporal-plan.md`_

**Date:** 2026-07-02 **Status:** implementation plan — **S1-2 / S1-3 / S1-4a / S1-4b landed** on `main`; S1-4c+ N-frame timeline still **blocked** (see [S-line graduation review](2026-07-04-scan-graduation-review.md)) **Companion spec:** [GAN implementation spec](2026-07-03-scan-temporal-core-landed.md)（产品目标、评估 rubric、风险登记 — 本文档侧重工程切片清单） **Prerequisite:** S0 charter (`scan-frame-v0` landed in `crates/auv-scan`) **Owner brief:** minimal real producer → inspect reader → multi-frame/temporal (only after single-frame loop is stable) > Generated from owner intent: S1-2 producer, S1-3 reader, S1-4 multi-frame de…

### AUV Scan S1 Slice 1: Frame + Artifact Contract — Implementation Handoff

_Source: `2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md`_

**Date:** 2026-07-02 **Status:** landed — `scan-frame-v0` wire + hermetic single-frame fixture **Prerequisite:** [S0 charter](2026-07-02-scan-charter.md), [S1 temporal core plan](2026-07-03-scan-temporal-core-landed.md) step 1

### AUV Scan S1 Slice 2: Producer Wiring — Implementation Handoff

_Source: `2026-07-02-auv-scan-s1-slice2-producer-handoff.md`_

**Date:** 2026-07-02 **Status:** landed — fixture-first producer + shared artifact bundle writer **Prerequisite:** [S1 slice 1 handoff](2026-07-03-scan-temporal-core-landed.md), [S1-2/3/4 plan](2026-07-03-scan-temporal-core-landed.md)

### AUV Scan S1 Slice 3: Read-side Consume — Implementation Handoff

_Source: `2026-07-02-auv-scan-s1-slice3-read-side-handoff.md`_

**Date:** 2026-07-03 **Status:** implemented — crate-local reader for `scan-frame-v0` artifact directories **Prerequisite:** [S1 slice 1 handoff](2026-07-03-scan-temporal-core-landed.md), [S1 slice 2 handoff](2026-07-03-scan-temporal-core-landed.md), [S1-2/3/4 plan](2026-07-03-scan-temporal-core-landed.md)

### AUV Scan S1-4a: Multi-frame Artifacts — Implementation Handoff

_Source: `2026-07-02-auv-scan-s1-s4a-multi-frame-handoff.md`_

**Date:** 2026-07-03 **Status:** landed — two-frame fixture producer + replay **Prerequisite:** [S1 slice 2 handoff](2026-07-03-scan-temporal-core-landed.md), [S1 slice 3 handoff](2026-07-03-scan-temporal-core-landed.md)

### AUV Scan S1-4b: Two-frame Motion / Timeline — Implementation Handoff

_Source: `2026-07-03-auv-scan-s1-s4b-motion-timeline-handoff.md`_

**Date:** 2026-07-03 **Status:** landed **Prerequisite:** [S1-4a multi-frame handoff](2026-07-03-scan-temporal-core-landed.md)

### AUV Scan S4 Lifecycle Evaluator v1 — Handoff

_Source: `2026-07-02-auv-scan-s4-lifecycle-evaluator-handoff.md`_

**Date:** 2026-07-03 **Status:** landed

### AUV Scan S7: Invoke Frame Producer — Implementation Handoff

_Source: `2026-07-06-auv-scan-s7-invoke-frame-producer-handoff.md`_

**Date:** 2026-07-06 **Status:** landed — `scan.frame` fixture-first invoke path writes bounded scan artifacts into runs **Prerequisite:** [S1 slice 2 producer handoff](2026-07-03-scan-temporal-core-landed.md), [S-line graduation review](2026-07-04-scan-graduation-review.md)


## Full durable notes (restored)

Active design vocabulary should prefer these full notes over the folded summary above:

- [`2026-07-02-scan-gan-spec.md`](2026-07-02-scan-gan-spec.md)
- [`2026-07-02-scan-charter.md`](2026-07-02-scan-charter.md)
- [`2026-07-04-scan-graduation-review.md`](2026-07-04-scan-graduation-review.md)

## Evidence / follow-ups

Open the absorbed source tombstones only if you need the pre-merge wording. Update new work against this merged note and the owning folder `INDEX.md`.
