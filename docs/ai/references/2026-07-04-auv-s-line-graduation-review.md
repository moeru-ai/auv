# 2026-07-04 AUV S-Line graduation review / state-of-lane audit

Date: 2026-07-04

Status: design-only graduation review on `main` @ `49cb750`. Covers S-line
(S0–S6 substrate roadmap, `crates/auv-scan` hermetic stack, and B-line bridge
through S6b-1). **No code extraction, no new implementation slices approved.**

Evidence harness re-run on audit date:

```text
cargo fmt --check          # pass
cargo check -p auv-scan    # pass
cargo test -p auv-scan     # 70 passed
cargo test scene_state_read  # 5 passed (root crate, S6b-1)
```

## Scope boundary

**In scope:**

- Re-grade S-line against
  [`2026-07-03-s-line-streaming-observation-substrate.md`](2026-07-03-s-line-streaming-observation-substrate.md)
  first acceptance batch and S0–S6 roadmapping labels.
- State-of-lane audit: what is landed, partial, missing, or correctly deferred.
- B-line bridge status through S6a (in-crate L3) and S6b-1 (`inspect_run` text).
- Boundary audit: scroll_scan, M/G, B semantic invention, naming collisions.

**Out of scope (explicit defer):**

- Rust production changes in `crates/auv-scan` or root runtime.
- `inspect_server` / viewer expansion (S6b+).
- Graduating `scan-scene-state-input-v0` to durable contract or
  `docs/TERMS_AND_CONCEPTS.md`.
- A-line (SceneBridge / NetEase ViewMemory) reopening.
- M-line (SLAM, 3DGS) and G-line (game telemetry) implementation.
- Qodana / CI configuration.

**Primary inputs (read-only):**

- [`2026-07-03-s-line-streaming-observation-substrate.md`](2026-07-03-s-line-streaming-observation-substrate.md)
- [`2026-07-02-auv-scan-s0-charter.md`](2026-07-02-auv-scan-s0-charter.md)
- Sixteen `*auv-scan*` reference handoffs/plans (see [Handoff reconciliation](#handoff-reconciliation))
- `crates/auv-scan/src/` — 13 public modules, 70 in-crate tests
- `src/scene_state_read.rs`, `src/inspect_scene_state.rs` — S6b-1 bridge

## Glossary (naming collision guard)

| Label | Meaning |
| --- | --- |
| **Substrate S6** | Roadmap stage: optional model backends (SLAM, 3DGS, telemetry adapters). **Deferred.** |
| **S6a / S6b** | B-line consumption slices (L3 inspect projection, run-read text). **Unrelated** to substrate S6. |
| **S-line observation read-model v1** | Recommended **external name** for what is landed today (hermetic contracts + in-memory read models). |
| **Streaming observation substrate** | Roadmap **target** — not claimed as graduated by this review. |

## Verdict vocabulary (aligned with Core-A)

| Label | Meaning |
| --- | --- |
| `landed proof` | Hermetic fixtures + tests satisfy the named gate; bounded owning crate |
| `candidate for narrow contract graduation` | Review language only — one wire/cluster may stop re-litigating "does S1 round-trip?" |
| `helper proof only` | Useful evidence; does not lift whole-line graduation |
| `partial` | Shape or evaluator exists; durable wire, runtime path, or end-to-end chain missing |
| `hold` | Do not graduate; intentional gap documented |
| `defer` | Correctly out of scope for current slice |
| `pass` | Guardrail satisfied (e.g. M/G not creeping into S core) |

**Landed ≠ streaming substrate graduated ≠ next slice approved.**

---

## Overall verdict

| Surface | Verdict |
| --- | --- |
| **Whole S-line as streaming observation substrate** | **`hold`** |
| **S1 contract cluster** (`scan-frame-v0` + producer + reader + two-frame batch) | **`candidate for narrow contract graduation`** (review language) |
| **S2 minimal temporal** (motion, association, `scan-timeline-v0`) | **`helper proof only`** |
| **S3–S5 product stack** | **`hold`** (`partial` evidence — in-memory read models) |
| **B-line bridge (S6a + S6b-1)** | **`partial`** — CLI text only; no viewer / `inspect_server` |
| **Substrate S6 (model backends)** | **`defer`** (clean — no creep) |

**Recommended external naming:** **S-line observation read-model v1 (hermetic)** until
runtime producer, observation-in-frame closure, and durable S3–S5 wires land.

---

## Gate matrix (S0–S6 + acceptance batch)

| Gate / stage | Substrate expectation | Status | Evidence |
| --- | --- | --- | --- |
| **S0** charter & five questions | Vocabulary + auditable question set | `landed proof` (docs) | [S0 charter](2026-07-02-auv-scan-s0-charter.md); `SceneDraftAnswers` in `scene_state.rs` |
| **S1** frame binding & artifact | Versioned `scan-frame-v0`, round-trip, strict validation | `landed proof` | [slice1](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md)–[slice3](2026-07-02-auv-scan-s1-slice3-read-side-handoff.md), [s4a](2026-07-02-auv-scan-s1-s4a-multi-frame-handoff.md); `artifact.rs`, `frame.rs`, `producer/`, `reader.rs` |
| **S1** roadmap extras | `quality_flags`, surface binding, pose, latency envelope | `hold` / missing | `ScanFrame` has bounds fields only — no `quality_flags` or `surface_ref` on wire |
| **S2** two-frame motion | Explicit estimate or `motion_unknown` | `landed proof` | `motion.rs`, `timeline.rs` ([s4b](2026-07-03-auv-scan-s1-s4b-motion-timeline-handoff.md)); metadata `window_bounds` delta |
| **S2** association | Stable identity + ambiguity diagnostic | `landed proof` | `association.rs`; `association_stable_v0`, `association_ambiguous_v0` fixtures |
| **S2** N-frame / durable tracks | Bounded sequence motion; `scan-tracks` wire | `hold` | `DIAG_UNSUPPORTED_FRAME_COUNT` when frame count ≠ 2; tracks wire not implemented |
| **S3** coverage ledger | Regions, freshness, negative evidence, completeness | `partial` | `coverage.rs` → `CoverageView` (in-memory); not substrate `CoverageLedger` wire |
| **S4** anchor lifecycle | observed → stale → reacquired \| lost with evidence | `partial` | [lifecycle evaluator](2026-07-02-auv-scan-s4-lifecycle-evaluator-handoff.md); baked `lifecycle_events` — not hot-path derived |
| **S5** scene state product | Answer S0 five questions without re-running scanner | `partial` | [S5a handoff](2026-07-03-auv-scan-s5-scene-state-handoff.md); `observations_by_frame` external to frame wire |
| **S5** durable product wire | `scan-scene-state-v0` | `defer` | Explicit non-goal in S5/S6 handoffs |
| **S6** model backends (substrate) | Cold-path SLAM / 3DGS / telemetry | `defer` | No pose/SLAM/3DGS in `auv-scan` |
| **Keyframes** (pipeline step) | `ScanKeyframe` selection | `hold` | Roadmap types only — no implementation |
| **B-line bridge** | Consume summary product, not raw frame JSON | `partial` | [S6a](2026-07-03-auv-scan-s6a-scene-state-inspect-handoff.md), [S6b-1](2026-07-03-auv-scan-s6b-scene-state-run-read-handoff.md); no viewer |
| **M/G guardrail** | Optional cold-path only | `pass` | Zero game/SLAM coupling in S core |
| **Runtime invoke** | Implicit run recording writes scan artifacts | `hold` | No catalog scan command; S6b-1 uses test-staged `scan-scene-state-input-v0` |
| **TERMS_AND_CONCEPTS** | Durable vocabulary for locked wires | `hold` | S0 TODO unfulfilled; provisional staging wire excluded |

### First acceptance batch (substrate doc)

| Acceptance gate | Verdict |
| --- | --- |
| S1 frame contract | ✅ `landed proof` |
| S2 two-frame motion | ✅ `landed proof` (metadata-proxy motion) |
| S2 association | ✅ `landed proof` |
| S3 coverage ledger | ⚠️ `partial` |
| S4 anchor lifecycle | ⚠️ `partial` |
| S5 read-side product | ⚠️ `partial` |
| B-line bridge | ⚠️ `partial` |
| M/G guardrail | ✅ `pass` |

---

## S0 five questions — per-question verdict

| # | S0 question | Score | Verdict | Fixture / code evidence |
| --- | --- | ---: | --- | --- |
| Q1 | What is visible in the current viewport? | 3/5 | `partial` | `TrackSceneSummary.latest_observation_present`; observations injected via `SceneStateInput`, not produced from `scan-frame-v0` |
| Q2 | Which visible objects are the same across frames? | 4/5 | `helper proof only` | `associate_adjacent_frames` → `IdentityAssessment`; `scene_ambiguous_v0` |
| Q3 | How did the viewport move? | 2/5 | `helper proof only` | `estimate_viewport_motion` / `scan-timeline-v0`; S5a: motion does not drive readiness |
| Q4 | Why was a target lost or reacquired? | 4/5 | `partial` | `evaluate_lifecycle` + lifecycle fixtures; events are fixture-baked |
| Q5 | Which conclusions are trustworthy, weak, stale, or ambiguous? | 4/5 | `partial` | `action_readiness.blocking_codes`, `SceneDiagnostic`; no durable product wire |

---

## Boundary audit

| Risk | Finding |
| --- | --- |
| **scroll_scan coupling** | ✅ Clean — `auv-scan` has zero `scroll_scan` imports; complementary donor only (S0 charter). |
| **M/G creep** | ✅ Clean — no SLAM, pose telemetry, Minecraft, or 3DGS in S core. |
| **B-line inventing semantics** | ✅ Mostly clean — B consumes `auv_scan::SceneStateInspect`; does not parse raw `scan-frame-*.json` ad hoc. ⚠️ Provisional `scan-scene-state-input-v0` lives in root crate (acceptable for S6b-1; blurs ownership). |
| **Observation split** | ⚠️ Intentional seam — weakens "answer five questions from frame artifacts alone" until observations persist or derive from frames. |
| **Motion semantics** | ⚠️ Thin proof — `window_bounds` delta passes two-frame gate; not production scroll/pan detection. |
| **Completion illusion** | ⚠️ 70 green tests + many "landed" handoffs mask S3–S5 non-durable state. |

No hard boundary **violations** found.

---

## B-line bridge status

| Layer | Status | Notes |
| --- | --- | --- |
| L2 `SceneStateProduct` | `landed proof` | `build_scene_state_product`, 6 scene fixtures |
| L3 `SceneStateInspect` | `landed proof` | `format_scene_state_inspect_text` — no `Serialize` |
| S6b-1 `inspect_run` text | `landed proof` | `scene_state_read.rs` — 5 tests |
| `inspect_server` | `hold` | No scene-state routes (scroll_scan only) |
| `inspect_server_viewer.html` | `hold` | No scene-state UI |
| Durable `scan-scene-state-v0` | `defer` | NOTICE in S6b-1: staging wire is provisional |
| Runtime producer for staging wire | `hold` | No invoke path writes `scan-scene-state-input-v0` |

---

## Handoff reconciliation

Cross-check of `*auv-scan*` references vs code on `49cb750`:

| Document | Declared status | Audit |
| --- | --- | --- |
| [S0 charter](2026-07-02-auv-scan-s0-charter.md) | design charter | ✅ Accurate |
| [S1 temporal core plan](2026-07-02-auv-scan-s1-temporal-core-plan.md) | step 1 landed | ✅ Accurate |
| [S1 slice1 handoff](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md) | landed | ✅ Matches `scan-frame-v0` |
| [S1 slice2 handoff](2026-07-02-auv-scan-s1-slice2-producer-handoff.md) | landed | ✅ Matches `producer/` |
| [S1 slice3 handoff](2026-07-02-auv-scan-s1-slice3-read-side-handoff.md) | implemented | ✅ Matches `reader.rs` |
| [S1 s4a handoff](2026-07-02-auv-scan-s1-s4a-multi-frame-handoff.md) | landed | ✅ `two_frame_v0` fixture |
| [S1 s4b handoff](2026-07-03-auv-scan-s1-s4b-motion-timeline-handoff.md) | landed | ✅ `scan-timeline-v0` |
| [S4 lifecycle evaluator](2026-07-02-auv-scan-s4-lifecycle-evaluator-handoff.md) | landed | ✅ `lifecycle.rs` |
| [S4 charter](2026-07-03-auv-scan-s4-anchor-lifecycle-charter.md) | docs-only | ✅ Accurate |
| [S5 charter](2026-07-03-auv-scan-s5-scene-state-charter.md) | docs-only | ✅ Accurate |
| [S5a handoff](2026-07-03-auv-scan-s5-scene-state-handoff.md) | landed | ✅ `scene_state.rs` |
| [S6a handoff](2026-07-03-auv-scan-s6a-scene-state-inspect-handoff.md) | landed | ✅ `scene_state_inspect.rs` |
| [S6b-1 handoff](2026-07-03-auv-scan-s6b-scene-state-run-read-handoff.md) | landed | ✅ `scene_state_read.rs` |
| [S1 s2-s4 engineering plan](2026-07-02-auv-scan-s1-s2-s4-producer-read-temporal-plan.md) | **was stale** (`not started`) | ⚠️ **Updated** — S1-2/3/4a/4b landed; see doc status line |
| [S1 s2-s4 GAN spec](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md) | rubric companion | ✅ Still valid as evaluation spec |
| [S-line substrate roadmap](2026-07-03-s-line-streaming-observation-substrate.md) | direction note | ✅ Accurate target; not claiming completion |

---

## Falsifiers

Evidence that would **downgrade** or **block** current conclusions:

| Falsifier | Would change |
| --- | --- |
| `scan-frame-v0` golden fixtures fail round-trip on `main` | S1 `candidate for narrow contract graduation` → `hold` |
| `cargo test -p auv-scan` drops below 70 without documented slice shrink | Hermetic proof regression — block any graduation language |
| Observations remain permanently outside frame wire with no documented boundary | S5 "read from artifacts" claim stays `partial` permanently — whole-line stays `hold` |
| `window_bounds` motion promoted to drive S5 readiness without visual-motion proof | S2 motion gate misrepresented — reopen S2 scope |
| `scan-scene-state-input-v0` written to TERMS without owner slice | Contract violation — revert terminology promotion |
| `scroll_scan` or game telemetry imported into `auv-scan` | Boundary violation — block graduation until removed |
| B-line parses raw frame JSON instead of `SceneStateInspect` | B semantic invention — reopen B bridge review |

Evidence that would **upgrade** (future review only, not automatic):

| Trigger | Possible upgrade |
| --- | --- |
| Durable `scan-coverage-v0` + golden reader | S3 `partial` → `landed proof` for coverage wire |
| Runtime invoke writes `scan-frame-*` into real runs | Runtime `hold` → `partial` bridge |
| `inspect_server` consumes `SceneStateListSummary` without new semantics | B-line `partial` → `landed proof` for inspect consumption |
| N-frame timeline fixture (3+ frames) with regression on two-frame tests | S2 `helper proof only` → stronger temporal proof |

---

## Recommended next slices (owner-approved only)

Priority order — **registration only; this review does not approve implementation:**

1. **S1 wire gap closure** — Add minimal `quality_flags` / binding metadata to `scan-frame-v0` with golden migration, **or** add `NOTICE` deferrals at `ScanFrame` for each roadmap field intentionally omitted.
2. **S3 durable `scan-coverage-v0`** — First real ledger artifact beside frame dir; golden round-trip.
3. **Invoke/runtime frame producer** — Catalog command writing `scan-frame-*.json` into implicit run storage via existing producer APIs (`live-capture` feature-gated).
4. **S1-4c N-frame adjacent timeline** — Extend `build_scan_timeline_from_bundle` beyond two frames; 3+ frame fixture.
5. **B-line S6b+** — `inspect_server` / viewer consumption of `SceneStateListSummary` when owner explicitly approves inspect surface expansion.

---

## Sign-off checklist (owner)

- [ ] Must-pass harness green on cited SHA (`49cb750` or successor)
- [ ] Accept **`hold`** on whole-line streaming substrate graduation
- [ ] Accept **`candidate for narrow contract graduation`** language for S1 cluster only
- [ ] Accept external name **S-line observation read-model v1 (hermetic)**
- [ ] Acknowledge `scan-scene-state-input-v0` is **provisional** — not TERMS / not durable contract
- [ ] Acknowledge S3–S5 are **in-memory read models** until durable wires land
- [ ] Pick zero or more items from [Recommended next slices](#recommended-next-slices-owner-approved-only) as named owner slices

---

## Explicit defer list

| Item | Trigger to reopen |
| --- | --- |
| Whole-line substrate graduation | Runtime producer + observation-in-frame closure + durable S3–S5 wires + B inspect consumption |
| `scan-scene-state-v0` durable wire | Owner-approved wire design + golden fixtures + TERMS entry |
| `TERMS_AND_CONCEPTS` scan terms | Owner locks wire names in an approved slice |
| Substrate S6 model backends | S1–S5 stable enough; cold-path slice with explicit M/G boundary |
| S1-4c+ multi-segment timeline | Owner names N-frame slice after S1-4b stable |

---

## Validation appendix

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
cargo test scene_state_read
git diff --check
```

**Fixture matrix for manual sign-off:**

- `crates/auv-scan/tests/fixtures/scan/scene/scene_*_v0/`
- `crates/auv-scan/tests/fixtures/scan/lifecycle/lifecycle_*_v0/`
- `crates/auv-scan/tests/fixtures/scan/association/association_*_v0/`
- `crates/auv-scan/tests/fixtures/scan/temporal/two_frame_v0/`

**Optional product demo (non-gate):** synthetic run with `scan-scene-state-input-v0` artifact →
`cargo run --quiet -- inspect run <run_id>` includes `Scene state:` block with `[scene.*]` sections.

---

## Related references

- S-line roadmap:
  [`2026-07-03-s-line-streaming-observation-substrate.md`](2026-07-03-s-line-streaming-observation-substrate.md)
- Graduation review pattern:
  [`2026-06-28-auv-core-a2-stage-quality-graduation-review.md`](2026-06-28-auv-core-a2-stage-quality-graduation-review.md)
- A-line completion context:
  [`2026-06-30-auv-scenebridge-a8-proof-graduation.md`](2026-06-30-auv-scenebridge-a8-proof-graduation.md)

## One-sentence summary

S-line has a **solid S1 hermetic foundation** (70 tests, clean M/G/scroll_scan boundaries) and
**minimal S2 proof**, but S3–S5 remain **in-memory evaluators**, B consumption is
**CLI text-only**, and there is **no runtime invoke path** — **hold** whole-line substrate
graduation; **split** a narrow **S1 contract graduation** candidate from substrate completion.
