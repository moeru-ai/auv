# 2026-06-27 AUV core spatial result consumption pattern

Date: 2026-06-27

Status: design-only core abstraction note. No runtime extraction, crate split, or
public API change is introduced by this document.

## Why this note exists

Minecraft MC-10 through MC-17 have now closed a coherent consumption chain over
persisted result artifacts:

```text
result artifact
→ semantic gate
→ spatial query
→ action-facing readiness view
→ witness selection
→ quality measurement
```

That chain is no longer Minecraft-specific in shape. The current risk is not
missing one more vertical feature; the risk is allowing the Minecraft naming and
slice order to harden into fake core by accident.

This note exists to freeze the reusable **pattern** before any code extraction:

- what the reusable stages are
- what each stage is allowed to consume and emit
- which parts are producer-side vs derived read-side
- what should stay app-specific for now
- what evidence is required before a future core graduation

## Non-goals

This note does **not**:

- extract a new core crate
- rename Minecraft modules
- add new CLI commands
- define a generic viewer UI
- define Gaussian inference APIs
- add action wiring or execution leases
- declare Minecraft code already graduated to core

If a future slice wants code extraction, it must cite this note and name the
exact owner-approved extraction boundary.

## The pattern

The reusable chain is:

```text
Producer Artifact
  → Semantic Gate
  → Spatial Query
  → Action Readiness View
  → Witness Artifact
  → Quality Measurement
```

This chain is **consumption-first**:

- it starts from persisted or replayable result artifacts
- it produces inspectable, auditable evidence at every stage
- it does not assume a specific model family, app vertical, or input backend

The chain is also **layered**:

- some stages are persisted producer outputs
- some stages are derived read models only
- some stages are evidence-only witnesses, not verdicts

That distinction matters more than the current Minecraft file names.

## Stage definitions

### 1. Producer Artifact

**Question answered**

```text
What durable result package do later stages consume?
```

Producer artifact is the persisted, lineage-carrying result package that later
stages read. In Minecraft this is the normalized training-result artifact layer
closed before MC-10.

Reusable invariant:

- the producer artifact owns lineage to upstream generation
- downstream stages consume this artifact instead of reopening earlier command
  inputs directly
- downstream stages may copy lineage fields, but they must not silently invent
  new upstream truth

This stage is a persistence boundary, not a quality judgment.

### 2. Semantic Gate

**Question answered**

```text
Is this producer artifact structurally consumable for the next semantic stage?
```

Semantic gate is the first typed consumer over the producer artifact. It checks
that the result package is structurally meaningful for downstream use.

Reusable invariant:

- semantic gate reads one persisted producer artifact entrypoint
- semantic gate validates structure and declared compatibility
- semantic gate does **not** grade usefulness or outcome quality

Expected status shape:

- `ready`
- `blocked`
- `failed`

Expected output shape:

- copied lineage
- structural findings
- explicit status and reason
- known limits

Minecraft mapping:

- MC-10 semantic validation

Core graduation signal:

- another vertical needs the same “artifact is structurally consumable” step
  with the same status semantics and lineage discipline

### 3. Spatial Query

**Question answered**

```text
Can a semantic-ready result answer a target-conditioned spatial query?
```

Spatial query is not Minecraft-specific in principle. It is a contract that
takes a semantic-ready result and a target/query intent, then returns an
inspectable answer or an honest failure layer.

Reusable invariant:

- spatial query consumes semantic-ready artifacts only
- query backends may be reference, provider, or hybrid
- backend comparison must stay explicit
- “answered but non-clickable” must remain distinct from “failed”

Expected output shape:

- query identity
- selected answer
- backend provenance
- comparison verdict when multiple backends answer
- copied lineage and known limits

Minecraft mapping:

- MC-12 query contract
- MC-15 checkpoint-native provider seam

Core graduation signal:

- at least one non-Minecraft vertical needs target-conditioned answer contracts
  with provider/reference comparison and the same honest status split

### 4. Action Readiness View

**Question answered**

```text
Can an existing query answer be consumed by action-facing code?
```

Action readiness is a **derived read model**, not a persisted producer artifact.
It exists to let action code consume query results without rereading raw query
contracts every time.

Reusable invariant:

- derives from an existing persisted query result
- does not create new truth
- does not dispatch actions
- does not upgrade failure into readiness

Expected output shape:

- `click_ready`
- `answer_non_clickable`
- `not_consumable`

Optional output:

- projected action point
- refusal reason

Minecraft mapping:

- MC-14 derived action-readiness view

Core graduation signal:

- multiple verticals need the same “consume answer for action, but do not act
  yet” abstraction

### 5. Witness Artifact

**Question answered**

```text
What concrete evidence frame / basis / comparison scene witnesses the result lineage?
```

Witness artifact records the concrete evidence item that later comparison or
quality work refers to. This stage is still not a usefulness verdict.

Reusable invariant:

- witness selection must name the authoritative evidence source
- downstream quality stages must not silently re-select witness inputs
- witness artifacts exist to anchor later measurement and human audit

Expected output shape:

- selected witness identity
- basis artifact identity
- evidence paths / references
- copied lineage
- known limits

Minecraft mapping:

- MC-16 holdout preview / checkpoint basis witness

Core graduation signal:

- another vertical needs a persisted “this exact evidence item witnesses the
  result lineage” artifact before quality or execution can proceed honestly

### 6. Quality Measurement

**Question answered**

```text
What evidence-bearing measurements can we compute against the witness artifact?
```

Quality measurement is deliberately narrower than “quality verdict”.

Reusable invariant:

- quality measurement consumes an authoritative witness artifact
- first version should emit measurement evidence, not threshold truth
- measurement policy must stay explicit about alignment, resizing, and omitted
  metrics
- quality measurement does not imply action promotion

Expected output shape:

- measurement backend label
- witness linkage
- raw or summarized metrics
- `measured_only | metric_partial | blocked | failed`
- known limits

Minecraft mapping:

- MC-17 holdout render quality evidence

Core graduation signal:

- at least one additional vertical needs witness-bound quality evidence with
  the same blocked/failed/measured distinction

## Ownership split

The pattern above should not be extracted as one giant “spatial intelligence”
module. Ownership is split by stage type.

### Persisted producer-side stages

These produce durable artifacts and usually belong in app/domain crates first:

- Producer Artifact
- Semantic Gate
- Spatial Query
- Witness Artifact
- Quality Measurement

They own:

- artifact schema
- lineage persistence
- status and reason contracts
- runtime execution and staging

### Derived read-side stages

These should remain consumer-side and derived unless a strong reason appears:

- Action Readiness View

They own:

- derived summaries
- inspect/read-side rendering
- viewer/read-model adaptation

They should not:

- back-write new producer truth
- introduce fake persistence to look symmetrical

## Cross-stage invariants

These are the real reusable core rules. If future verticals share these rules,
they are candidates for core extraction.

### A. Single authoritative upstream artifact per stage

Each stage should have one explicit upstream artifact entrypoint.

Bad pattern:

```text
semantic reads producer
query reopens producer + earlier launch inputs + local env
quality reopens witness and reselects frame from scratch
```

Good pattern:

```text
semantic reads producer
query reads semantic
witness reads semantic
quality reads witness + semantic cross-check
```

### B. Business lineage beats path coincidence

Pairing and cross-checking should rely on business lineage fields, not output
directory coincidence or artifact path shortcuts.

Minecraft already established this in MC-11 / MC-13 / MC-16 / MC-17 read-side
consumers. That rule is core-worthy.

### C. Evidence stages must stay honest

A stage that only records evidence must not silently upgrade itself into a
verdict stage.

Examples:

- witness is not quality
- quality measurement is not usefulness verdict
- action readiness is not action dispatch

### D. `blocked` vs `failed` must remain meaningful

The whole chain relies on an honest split:

- `blocked` = upstream precondition missing or intentionally unavailable
- `failed` = attempted stage execution or contract parse failed

If future extractions blur that line, inspect truth degrades quickly.

### E. Provider seam is not model truth

Provider-backed answers are not automatically “real” or “better” than reference
answers. The contract must continue to record:

- which backend answered
- what comparison verdict exists
- what remains deferred

This matters far beyond Minecraft.

## What is core-worthy vs not yet

### Likely core-worthy later

- stage vocabulary and status discipline
- lineage-based pairing rules for read-side consumers
- query/provider/reference comparison vocabulary
- derived action-readiness vocabulary
- witness-vs-quality separation

### Not core-worthy yet

- Minecraft block target semantics
- scene-packet frame JSON specifics
- nerfstudio / checkpoint file assumptions
- holdout screenshot path conventions
- current command-line argument shapes
- current viewer card layouts

### Needs more evidence before extraction

- generic query answer schema
- generic witness schema
- generic quality measurement schema
- provider arbitration APIs

Those may be right later, but current evidence still comes from one vertical.

## Proposed terminology

These names are provisionally better than Minecraft-shaped names for future core
discussion:

- `ResultArtifact`
- `SemanticGateResult`
- `SpatialQueryResult`
- `ActionReadinessView`
- `WitnessArtifact`
- `QualityMeasurementArtifact`
- `ProviderComparisonVerdict`

This note is not approval to rename code now. It is a naming target for future
core slices.

## Recommended next slices

### Core-A D2 — terminology and seam admission note

Delivered in:
`docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-admission-table.md`

That note classifies concrete Minecraft modules and symbols as:

- keep app-specific
- extract helper only
- candidate core contract
- explicitly deferred

### Core-A D3 — terms document update

Delivered in `docs/TERMS_AND_CONCEPTS.md` with minimal new sections for:

- semantic gate
- witness artifact
- quality measurement
- action readiness view

Do not dump the whole Minecraft roadmap into terms.

### Core-A D4 — candidate contract proof matrix

Delivered in:
`docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`

That note defines, for each candidate contract:

- what second-vertical evidence is still missing
- what falsifier would stop graduation
- what the smallest acceptable extraction shape would be
- what still disqualifies extraction

### Core-B — first code extraction

Only after at least one non-Minecraft consumer exists, or after the owner
explicitly wants cross-vertical reuse, consider extracting:

- status enums or shared result labels
- pairing helpers
- provider comparison helpers

Do **not** start with a giant generic runtime trait.

Design note (not yet implemented):
[`2026-06-27-auv-core-b2-dual-backend-query-compare-helper-extraction.md`](2026-06-27-auv-core-b2-dual-backend-query-compare-helper-extraction.md)
records the owner-approved Core-B2 helper-only plan for MC-12 dual-backend
query compare policy (#1).

## Explicit defer list

This note intentionally defers:

- generic render provider API
- generic action lease / dispatch protocol — still deferred; Core-C1 opens the
  **design boundary only** for one attempt vs pre-dispatch refusal (see
  [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-auv-core-c1-action-attempt-admission-design.md));
  it does **not** extract runtime, lease, or controller surfaces
- candidate promotion integration
- Gaussian-native inference abstraction
- threshold-based quality verdicts (opened only for MC-17 D3 derived read-side; see docs/ai/references/2026-06-27-minecraft-mc17-d3-quality-verdict-design.md)
- viewer unification
- public SDK surface

Those are different slices and should not be smuggled into “core cleanup”.

## Direct source slices

This pattern was derived from:

- MC-10 semantic validation
- MC-12 spatial query contract
- MC-14 action-facing derived readiness
- MC-15 checkpoint-native provider seam
- MC-16 holdout witness
- MC-17 quality measurement evidence

The pattern is real. Core extraction is still deferred on purpose.
