# 2026-07-05 AUV S1 bounded contract graduation review

Date: 2026-07-05

Status: design-only **bounded** contract graduation **review** on `60214d2` (**S1 frame +
S2 two-frame timeline wires only** — artifact/wire/IO). Extracts and freezes language from
[`2026-07-04-auv-s-line-graduation-review.md`](2026-07-04-auv-s-line-graduation-review.md).
**No code extraction, no TERMS promotion, no substrate whole-line graduation approved.**

## 1. Scope / parent dependency

### What this document is

This is **not** a second S-line state-of-lane audit. The parent review at `49cb750` already
graded S0–S6, B-line bridge, and whole-line substrate (`hold`).

This document **only** answers four boundary questions for the S1 cluster:

1. Which types/surfaces may use **`graduate bounded`** language?
2. Which remain **crate-local public API** and must not be externalized?
3. Which **read-side semantics** may be stated as stable external promises?
4. Which **falsifiers** block broader graduation language?

### Parent vs this review (SHA footnote)

| Document | Basis SHA | Harness |
| --- | --- | --- |
| Parent `2026-07-04` S-line graduation review | `49cb750` | 70 + 5 tests cited there |
| **This** bounded contract review | **`60214d2`** | Re-run below |

Evidence harness re-run on this review date:

```text
git rev-parse HEAD  → 60214d2c8582c1d9c96762c2313df17febd17d2d
cargo test -p auv-scan     → 70 passed
cargo test scene_state_read → 5 passed
```

If `HEAD` advances after this document lands, update **this** footnote only — do not rewrite
parent review verdicts.

### Parent vocabulary mapping

| Parent label ([2026-07-04](2026-07-04-auv-s-line-graduation-review.md)) | This review |
| --- | --- |
| `candidate for narrow contract graduation` (S1 cluster) | `graduate bounded` for `scan-frame-v0` artifact/wire/IO only |
| `helper proof only` (S2 minimal temporal) | **Unchanged** for motion/association substrate claims; `graduate bounded` for `scan-timeline-v0` **wire/IO only** (two-frame) |

### Parent rows left unchanged

Parent **Overall verdict** and **gate matrix** status cells are not rewritten here. Notably:

- **S2 minimal temporal** stays **`helper proof only`** for motion proof and temporal substrate
  language — this document does **not** upgrade S2 to `landed proof` for motion semantics.
- Timeline `graduate bounded` means frozen JSON wire + read/write reject paths — **not** visual-motion graduation.

**Pending owner sign-off:** Tables in §3 propose bounded language; they are not final promotion until §6 is checked.

### In scope

- Freeze **`graduate bounded`** language for `scan-frame-v0` and `scan-timeline-v0`
  artifact/wire/IO/reject semantics only.
- Inventory four candidate surfaces: `ScanFrame`, timeline, `scene_state`, inspect summary.
- Map every `auv_scan` crate-root re-export to graduation class.
- List stable promises and S1-only falsifiers.

### Out of scope (explicit defer)

- Rust production changes in `crates/auv-scan` or root runtime.
- Core extraction (`src/contract.rs`, `auv-tracing-driver` promotion).
- `docs/TERMS_AND_CONCEPTS.md` entries (unless owner opens a separate TERMS slice).
- Whole-line gate matrix, S0 five-question scoring, B-line viewer/`inspect_server`.
- Graduating `scan-scene-state-input-v0` or any `SceneStateProduct` wire.
- S7, runtime invoke producer, N-frame timeline, durable `scan-coverage-v0`.

### Verdict vocabulary

| Label | Meaning |
| --- | --- |
| `graduate bounded` | Artifact/wire/IO/reject semantics + golden evidence may use stable external language |
| `crate-local public API` | Exported from `auv_scan` but **not** a graduated contract; stays in owning crate |
| `hold` | No graduation; inventory/explain only; no cross-crate stability claim |
| `provisional staging` | Test/run bridge wire explicitly not durable |

**`graduate bounded` ≠ re-export ≠ core extraction ≠ final promotion.**

---

## 2. Candidate inventory

Four surfaces **synthesized from parent S1/S2/B-line evidence** (the parent review does not
list these four names as a single inventory):

| Surface | Owning module(s) | Durable JSON wire? | Default class |
| --- | --- | --- | --- |
| **ScanFrame** | `frame.rs`, `artifact.rs`, `reader.rs` (loader) | Yes — `scan-frame-v0` | Filter → `graduate bounded` |
| **Timeline** | `timeline.rs`, `motion.rs` (input) | Yes — `scan-timeline-v0` | Filter → `graduate bounded` (two-frame bounded) |
| **scene_state** | `scene_state.rs`, `association.rs`, `coverage.rs`, `lifecycle.rs` | No | `hold` |
| **inspect summary** | `scene_state_inspect.rs`, root `scene_state_read.rs` | No (`NOTICE`: no `Serialize`) | `hold` |

### Surface inventory (`auv_scan` crate root re-exports)

**Rule:** `lib.rs` re-export is **surface inventory**, not a graduation candidate list. Only
symbols passing the **durable wire filter** (schema id + `Serialize`/`Deserialize` wire types +
artifact read/write + documented reject paths + golden fixtures) enter `graduate bounded`.

#### `graduate bounded`

| Symbol | Wire / IO role |
| --- | --- |
| `SCAN_FRAME_SCHEMA_VERSION` | Schema gate (`"scan-frame-v0"`) |
| `ScanBounds`, `ScanImageRef`, `ScanFrame` | Wire types; `ScanFrame::validate_wire` |
| `ScanArtifactError` | Reject semantics for frame artifact IO |
| `frame_artifact_file_name`, `read_frame_artifact`, `write_frame_artifact` | `scan-frame-NNNN.json` IO |
| `SCAN_TIMELINE_SCHEMA_VERSION` | Schema gate (`"scan-timeline-v0"`) |
| `SCAN_TIMELINE_ARTIFACT_FILE_NAME` | Directory artifact name (`scan-timeline.json`) |
| `ScanTimelineWire`, `TimelineSegmentWire`, `TimelineMotionWire`, `TimelineDiagnosticWire` | Timeline wire shapes |
| `TimelineError` | Timeline artifact reject semantics |
| `DIAG_INSUFFICIENT_FRAMES`, `DIAG_UNSUPPORTED_FRAME_COUNT` | Two-frame bounded diagnostic codes |
| `read_timeline_artifact`, `write_timeline_artifact` | Timeline JSON IO |

#### `crate-local public API` (all other `lib.rs` re-exports)

| Group | Symbols |
| --- | --- |
| **Frame loader / inspect** | `ScanFrameBundle`, `ScanInspectError`, `load_scan_frames_from_dir`, `replay_scan_frames_from_dir`, `verify_frame_image_dimensions`, `summarize_scan_frame_text` |
| **Producer** | `FrameCaptureMeta`, `ProducedFrame`, `ProducedFrameBatch`, `ScanProducerError`, `bounds_to_scan_bounds`, `bounds_to_scan_bounds_f64`, `build_scan_frame`, `frame_from_capture`, `produce_frame_from_fixture_dir`, `produce_frames_from_fixture_dir`, `write_frame_with_image`, `produce_frame_from_capture` (`live-capture`) |
| **Motion** | `MotionError`, `MotionEstimate`, `MotionResult`, `MotionUnknown`, `estimate_viewport_motion` |
| **Association** | `AssociationDiagnostic`, `AssociationResult`, `FrameObservation`, `associate_adjacent_frames` |
| **Coverage** | `CompletenessClaim`, `CoverageEntry`, `CoverageView`, `NegativeEvidence`, `build_coverage_view` |
| **Lifecycle** | `LifecycleError`, `LifecycleEvent`, `LifecycleVerdict`, `TransitionEvidence`, `evaluate_lifecycle` |
| **Scene state (L2)** | `ActionReadiness`, `IdentityAssessment`, `ObservationRequest`, `SceneDiagnostic`, `SceneDraftAnswers`, `SceneStateError`, `SceneStateInput`, `SceneStateProduct`, `TrackSceneSummary`, `VisibilityAssessment`, `build_scene_state_product`, `summarize_scene_state_text` |
| **Inspect (L3)** | `SceneStateInspect`, `SceneStateListSummary`, `build_scene_state_inspect`, `format_scene_state_inspect_text`, `summarize_scene_state_inspect` |
| **Timeline builder / text** | `build_scan_timeline_from_bundle`, `format_scan_timeline_text` |

#### `provisional staging` (root crate, not `auv_scan` re-export)

| Symbol / constant | Location | Notes |
| --- | --- | --- |
| `SCENE_STATE_INPUT_ARTIFACT_ROLE`, `SCENE_STATE_INPUT_SCHEMA_VERSION` | `src/scene_state_read.rs` | `scan-scene-state-input-v0` — test/staging only per S6b-1 NOTICE |
| `SceneStateReadOutcome`, `build_scene_state_inspect_for_run`, `format_scene_state_read_text` | `src/scene_state_read.rs` | S6b-1 `inspect_run` text bridge |

---

## 3. Graduated vs hold (four-surface verdict)

**Proposed bounded language — pending owner sign-off (§6).**

Answers **Q1 — which types may graduate**:

| Cluster | Verdict | Frozen bounded boundary | Explicitly not graduated |
| --- | --- | --- | --- |
| **ScanFrame** | **`graduate bounded`** | `scan-frame-v0` field set; `scan-frame-NNNN.json`; `read`/`write_frame_artifact`; `validate_wire` + `ScanArtifactError` paths | Roadmap extras (`quality_flags`, surface binding, pose); observations not in frame wire |
| **Timeline** | **`graduate bounded`** | `scan-timeline-v0` wire + IO; **exactly two frames** → one adjacent segment or diagnostics; diagnostic codes above | N-frame timeline; `window_bounds` delta as visual-motion proof; not a run-level substrate envelope — parent S2 motion row stays **`helper proof only`** |
| **scene_state** | **`hold`** | Inventory only: `SceneStateProduct` is a crate-local read-model seam; hermetic tests prove usability | No durable wire; no cross-crate stability; `observations_by_frame` external to frame artifacts |
| **inspect summary** | **`hold`** | Inventory only: S6b-1 text bridge exists (`inspect_run`) | No wire; no section-order contract; no `SceneStateListSummary` cross-crate API; viewer/`inspect_server` zero consumption |

### Recommended external naming (unchanged from parent)

- Lane: **S-line observation read-model v1 (hermetic)** — whole line still `hold` as streaming substrate.
- Addendum after this review: **S1 bounded artifact contracts reviewed** — **not** "streaming substrate graduated".

---

## 4. Stable promises (read-side)

Answers **Q3 — which semantics may be promised externally**:

### May promise (S1 bounded package only)

| Promise | Mechanism | Evidence |
| --- | --- | --- |
| Frame schema gate | `schema_version != scan-frame-v0` → `ScanArtifactError::SchemaMismatch` | `frame.rs`, `artifact.rs` tests |
| Positive bounds | `width`/`height` ≤ 0 on `window_bounds` / `viewport_bounds` → `InvalidBounds` | `ScanFrame::validate_wire` |
| Frame file naming | `scan-frame-0001.json` … from `sequence_index` | `frame_artifact_file_name` |
| Directory frame load | Top-level `scan-frame-*.json` only; empty dir → `NoFramesFound` | `reader.rs` |
| Sequence discipline | Duplicate or non-monotonic `sequence_index` → `ScanInspectError` | `reader.rs` tests |
| Optional PNG check | `verify_frame_image_dimensions` when caller enables | `reader.rs` |
| Timeline two-frame gate | Frame count ≠ 2 → diagnostics, no silent segment fabrication | `timeline.rs`, `DIAG_*` |
| Timeline file name | `scan-timeline.json` beside frame dir (directory-level, not run envelope) | `SCAN_TIMELINE_ARTIFACT_FILE_NAME` |

**NOTICE:** Promises above are **artifact-on-disk / wire reject semantics**. Loader APIs such as
`load_scan_frames_from_dir` and `ScanInspectError` remain **`crate-local public API`** unless a
future owner-approved extraction slice says otherwise.

### Must not promise

| Claim | Why blocked |
| --- | --- |
| S0 five questions answerable from scan artifacts alone | Observations live in `SceneStateInput.observations_by_frame`, outside `scan-frame-v0` |
| Viewport motion proven | Timeline motion uses `window_bounds` metadata proxy |
| Scene state durable / stable cross-crate | No `scan-scene-state-v0`; `SceneStateProduct` has no `Serialize` |
| Inspect text / list summary stable | Formatters are implementation projections; S6b-1 staging is provisional |
| Streaming observation substrate graduated | Runtime producer, durable S3–S5, B-line viewer missing — parent `hold` |

**Deleted relative to draft plans:** no "crate-internal stability" column for `SceneStateProduct`
or S6a — inventory without graduation language.

---

## 5. Falsifiers (S1 bounded only)

Answers **Q4 — what blocks broader graduation**. Whole-line falsifiers (scroll_scan creep,
B-line semantic invention, etc.) remain in the
[parent review](2026-07-04-auv-s-line-graduation-review.md#falsifiers) — cross-reference only.

| Falsifier | Effect on this review |
| --- | --- |
| `scan-frame-v0` golden round-trip fails on cited SHA | ScanFrame `graduate bounded` → `hold` |
| `scan-timeline-v0` golden or two-frame semantic tests fail | Timeline `graduate bounded` → `hold` |
| Roadmap S1 gaps (`quality_flags`, binding, pose) undocumented while claiming full S1 graduated | Bounded language invalid — need wire-gap NOTICE slice first |
| `window_bounds` delta described as visual-motion proof | Timeline motion must be labeled helper-only; shrink graduation wording |
| N-frame timeline claimed without 3+ frame golden + two-frame regression | Block timeline expansion beyond two-frame bounded contract |
| `cargo test -p auv-scan` count drops below 70 without documented slice shrink | Block all bounded graduation language |
| `scan-scene-state-input-v0` promoted to TERMS or called durable | Contract violation — revert terminology; scene_state stays `hold` |
| Readers treat timeline `graduate bounded` as parent S2 **`landed proof`** for motion | Revert timeline graduation wording — wire/IO only; S2 motion stays **`helper proof only`** |
| Parent sign-off still unchecked while this doc's §6 is treated as approved | Bounded language is **proposed** until owner checks §6; parent `candidate…` row unchanged until sign-off |

### Upgrade triggers (future review only — not approved here)

| Trigger | Possible future upgrade |
| --- | --- |
| Wire-gap NOTICE or minimal `quality_flags` on `scan-frame-v0` | Tighten ScanFrame bounded boundary documentation |
| Durable `scan-coverage-v0` + golden | New wire graduation review — separate from S1 frame/timeline |
| Runtime invoke writes `scan-frame-*` into real runs | Parent runtime `hold` → `partial` — not automatic S1 promotion |
| N-frame timeline fixture + regression | Timeline bounded contract revision — separate slice |

---

## 6. Owner sign-off checklist

- [ ] Harness green on cited SHA (`60214d2` or successor) — 70 + 5 tests
- [ ] Accept **`graduate bounded`** for `scan-frame-v0` + `scan-timeline-v0` (two-frame) **artifact/wire/IO only**
- [ ] Accept **`hold`** for `scene_state` and `inspect summary` — no semi-stable API promises
- [ ] Accept **`crate-local public API`** default for all non-wire `auv_scan` re-exports
- [ ] Accept external naming: observation read-model v1 + **S1 bounded artifact contracts reviewed**
- [ ] Acknowledge parent whole-line substrate remains **`hold`**
- [ ] **TERMS sub-slice:** open separately? (default **no** — this review does not write TERMS)
- [ ] Pick zero or more follow-ups from parent [recommended next slices](2026-07-04-auv-s-line-graduation-review.md#recommended-next-slices-owner-approved-only) (wire-gap NOTICE, S3 coverage, runtime producer, N-frame timeline, B-line S6b+)

### Registered follow-ups (not approved by this document)

1. S1 wire gap closure — `quality_flags` / binding metadata **or** `NOTICE` deferrals on `ScanFrame`
2. S3 durable `scan-coverage-v0`
3. Invoke/runtime frame producer
4. S1-4c N-frame adjacent timeline
5. B-line S6b+ (`inspect_server` / viewer)

---

## Validation appendix

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
cargo test scene_state_read
git diff --check
```

**Golden / fixture pointers:**

- `crates/auv-scan/tests/fixtures/scan/` — frame, temporal/two_frame, association, lifecycle, scene
- Parent review fixture matrix for manual sign-off

---

## Primary inputs (read-only)

- [`2026-07-04-auv-s-line-graduation-review.md`](2026-07-04-auv-s-line-graduation-review.md)
- [`2026-06-28-auv-core-a2-stage-quality-graduation-review.md`](2026-06-28-auv-core-a2-stage-quality-graduation-review.md)
- [`2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md`](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md)
- [`2026-07-03-auv-scan-s1-s4b-motion-timeline-handoff.md`](2026-07-03-auv-scan-s1-s4b-motion-timeline-handoff.md)
- [`2026-07-03-auv-scan-s6b-scene-state-run-read-handoff.md`](2026-07-03-auv-scan-s6b-scene-state-run-read-handoff.md)
- `crates/auv-scan/src/lib.rs`, `frame.rs`, `timeline.rs`, `scene_state.rs`, `scene_state_inspect.rs`
- `src/scene_state_read.rs`
