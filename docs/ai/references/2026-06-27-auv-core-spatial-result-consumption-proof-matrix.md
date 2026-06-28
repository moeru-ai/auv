# 2026-06-27 AUV core spatial result consumption proof matrix

Date: 2026-06-27

Status: design-only graduation matrix. This note defines what evidence each
Core-A candidate contract still needs before any code extraction can be taken
seriously.

## Why this note exists

Core-A D2 already classified the Minecraft MC-10 through MC-17 surface into:

- keep app-specific
- extract helper only
- candidate core contract
- explicitly deferred

That still leaves one risk: people may read “candidate core contract” as
“basically ready, go extract it”.

This D4 note closes that gap. It says, for each candidate contract:

- what positive evidence is still required
- what counter-evidence or falsifier must be checked
- what the smallest acceptable extraction shape would be
- what would still disqualify extraction

## Non-goals

This note does **not**:

- extract code
- rename current Minecraft symbols
- introduce a new core crate
- approve helper extraction automatically
- broaden into generic provider traits or viewer unification

## Graduation rules that apply to every candidate

Before any candidate below can graduate into shared AUV code, all of these
must hold:

1. **Second-vertical evidence exists**
   - not another Minecraft slice
   - not a synthetic doc-only example
   - not just “this shape feels reusable”
2. **One positive and one negative path are both evidenced**
   - success-only evidence is not enough
   - the contract must preserve its honest failure split under pressure
3. **The extracted name is no longer donor-shaped**
   - no `minecraft`, `checkpoint`, `block`, `nerfstudio`, or equivalent donor
     vocabulary in the shared type
4. **The shared boundary is smaller than the donor implementation**
   - extract the contract or helper, not the whole vertical module
5. **An owning layer is explicit**
   - runtime contract, domain helper, or read-side helper must be named in
     advance

If any one of these fails, the candidate stays local.

## Candidate proof matrix

| Candidate contract | Current donor symbols | Positive evidence still required | Counter-evidence / falsifier still required | Smallest acceptable extraction shape | Current blockers | Current verdict |
| --- | --- | --- | --- | --- | --- | --- |
| Stage status triad | `TrainingResultSemanticStatus`, `HoldoutPreviewStatus`, `HoldoutRenderQualityStatus` | One non-Minecraft vertical must need the same `ready / blocked / failed` split across at least two persisted stages, with lineage-carrying artifacts. | Show a real case where one vertical needs a fourth state or where `blocked` and `failed` collapse without loss. If that happens, do not extract the triad yet. | Shared enum or label helper only; no shared manifest struct. | Only Minecraft donor evidence exists. No second vertical. | `candidate, not admissible yet` |
| Query status triad | `TrainingResultSpatialQueryStatus` | One non-Minecraft vertical must expose target-conditioned answers where `answered` must stay distinct from readiness or semantic success. | Show a real case where query answers are naturally boolean or where `answered` adds no information. If true, this stays app-local. | Shared query-status enum only. | Main matrix verdict unchanged pending owner acceptance of graduation review; osu adds probe-local recurrence only — see graduation review (admissibility-only, defer extraction). | `candidate, not admissible yet`¹ |
| Provider comparison verdict | `TrainingResultSpatialQueryComparisonVerdict` | Another vertical must actually run dual-backend answer compare and need the same `match / divergent / provider_only / reference_only / not_comparable` split in persisted evidence. | Show a real compare seam where ordering, partiality, or uncertainty needs extra states beyond the five-label set. | Shared compare-verdict enum or helper. | Only Minecraft provider/reference compare exists. | `candidate, not admissible yet` |
| Action readiness view | `TrainingResultSpatialQueryActionEligibility`, `TrainingResultSpatialQueryActionReadiness`, `derive_action_readiness`, `derive_minecraft_training_result_spatial_query_action_readiness` | Another vertical must need a **derived** action-facing view over persisted answer artifacts, with at least one `click_ready`-like path and one honest non-actionable path. | Show a real case where action-facing consumption must mutate producer truth or where dispatch and readiness cannot be kept separate. | Shared derived read-model contract only; no runtime dispatch wiring. | Main matrix verdict unchanged pending owner acceptance of graduation review; osu adds derived-shape recurrence only — capture-space, not dispatch-safe — see graduation review. | `candidate, not admissible yet`¹ |
| Quality measurement verdict | `HoldoutRenderQualityVerdict` | Another vertical must measure witness-bound quality evidence and need the same split between `measured_only`, `metric_partial`, `blocked`, and `failed`. | Show a real measurement seam where thresholds are inseparable from measurement evidence, or where partial measurement is meaningless. | Shared evidence-verdict enum only. | Only MC-17 photometric evidence exists, and it is single-vertical. | `candidate, not admissible yet` |
| Persisted backend label discipline | `HoldoutRenderQualityBackend`, `TrainingResultSpatialQueryBackend` | Another vertical must persist backend provenance and independently hit the same rule: stable backend labels belong in artifacts, raw runtime command text does not. | Show a real vertical where backend provenance cannot be represented by stable labels alone. | Shared label-discipline rule or tiny backend-label trait bound, if ever needed. | Current proof is policy-level only; no second donor. | `candidate, not admissible yet` |


## osu! second-vertical probe (2026-06-27)

Companion evidence:
`docs/ai/references/2026-06-27-auv-second-vertical-consumption-probe-osu-evidence.md`

This probe closes **MC-14-analog derived consumption** on osu! only (query→
readiness shape recurrence, not full MC-14 window-readiness parity). It does not
graduate types into core and does not claim witness, quality, or live-click
wiring.

| Candidate contract | Verdict after osu probe | Notes |
| --- | --- | --- |
| Query status triad | **satisfied as second-vertical probe-local recurrence** | `answered` distinct from semantic `ready`; single backend only — **not extraction-pressure evidence**. |
| Action readiness view | **satisfied as second-vertical probe-local recurrence** | Derived triad without dispatch; **capture-space consumability**, not click authority. |
| Stage status triad | **partial (structurally shallow)** | Semantic + query only; no staged-consumption family expansion — not near-complete partial. |
| Provider comparison verdict | **not satisfied** | Dual-backend compare intentionally deferred. |
| Quality measurement verdict | **out of scope** | Excluded by osu probe design. |
| Persisted backend label discipline | **partial (second vertical)** | `playfield_projection_reference` persisted; no second backend family yet. |

Rows marked probe-local remain **non-admissible for Core-B extraction**.
Graduation review for query status triad and action readiness view only:
`docs/ai/references/2026-06-27-auv-core-a-query-readiness-graduation-review.md`.

That review may record **helper-only admissible (admissibility language only)** for
those two rows. **Admissible does not mean recommended now**, does not mean the
next slice, and does not lift extraction pressure — default remains **defer**.

¹ See graduation review
`docs/ai/references/2026-06-27-auv-core-a-query-readiness-graduation-review.md`:
may record **helper-only admissible (review language only)** for these two rows
after owner acceptance — **not** an extraction recommendation; default **defer**.

## What does not need a second vertical

Not every reuse decision needs a full graduation process.

The following may extract earlier **as helpers only**, if repetition appears and
the extracted shape stays narrow:

- artifact JSON read helpers
- MIME / JSON gating helpers
- business-key unique-match helper
- narrow RGB metric math helper

These are still not automatic. The test is simpler:

- duplicated in more than one owned module
- extraction removes repetition without inventing a new project concept
- no donor-specific vocabulary leaks into the helper name

## Disqualifiers

Any of the following should stop a proposed graduation immediately:

- the shared type name is still Minecraft-shaped
- the extraction proposal moves whole manifests instead of the contract seam
- the proposal mixes runtime, provider, read-side, and viewer concerns together
- the proposal claims “likely reusable” without a second real consumer
- the proposal uses D2 candidate status as if it were extraction approval
- the proposal hides unresolved donor quirks behind a generic name

## Concrete next-step filter

If someone proposes Core-B extraction after this note, the first question is
not “is it elegant?”.

The first question is:

```text
Which exact row in the proof matrix is now satisfied by new evidence?
```

If that cannot be answered concretely, the extraction is premature.

## Relationship to Core-A D1–D3

- D1 froze the stage pattern:
  `docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-pattern.md`
- D2 classified actual modules and donor symbols:
  `docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-admission-table.md`
- D3 added the minimal vocabulary to:
  `docs/TERMS_AND_CONCEPTS.md`

D4 does not add more vocabulary. It adds graduation gates.
