# AUV View Parser Anchor Reacquisition v0

Date: 2026-05-29

Status: v0 algorithm spec for re-finding a previously observed
`ViewNode` from a stored `ViewMemory`. **Every threshold and stage
ordering decision below is marked `REVIEW(...)` because the
heuristics are starting points that must be tuned against real
captures before being treated as load-bearing.**

Audience: owner, reviewers, and any agent (Codex, Claude, others)
implementing the reacquisition entry point in
`auv-view::memory::reacquire` or wiring it into a follow-up
command (e.g. `playlist get <anchor>`).

## Purpose

The IR shapes spec defined `ViewAnchor` with a `ReacquireStrategy`
enum but deferred the matching algorithm. The ViewMemory v0 spec
defined what is persisted but not how to read it back into a usable
target. Without a reacquisition algorithm:

- `playlist get <anchor>` cannot re-find an item without running a
  full parse, so the memory layer adds disk write cost without
  read-side payoff.
- Two implementations of reacquisition land at different match
  rules, and the same anchor reads as "found" in one and "not
  found" in the other.

This spec pins the v0 algorithm. It is a cascade of decreasingly
strict matchers, each tied to a `ReacquireStrategy` variant. The
top-level outcome maps cleanly onto the diagnostic policy so
reacquisition failures produce the same kind of evidence as parse
failures.

## Relationship to other specs

```text
view-parser-ir-shapes-v0.md            ViewAnchor + ReacquireStrategy + ViewNodeId rules
view-parser-view-memory-v0.md           ViewMemory shape that this algorithm consumes
view-parser-diagnostic-policy-v0.md     diagnostic kinds reused below
view-parser-layer-contracts-v0.md       parsers produce reconstructions; reacquisition consumes them
view-parser-trace-layout-v0.md          span tree (extended here for reacquisition)
netease-sidebar-region-detection-v0.md  region detection still runs as a precondition
netease-playlist-item-parsing-v0.md     item parsing still runs in single-viewport mode
```

## What reacquisition is and is not

Reacquisition is **read-side**:

- Input: a `ViewMemory` (loaded per its freshness rules) and a
  target — either a `ViewNodeId`, an anchor id, or a label string
  with optional section hint.
- Output: a `ReacquireOutcome` carrying either a current-run
  `ViewNode` whose evidence is fresh enough for action, or a
  structured failure describing why the target could not be found.

It is **not** a full parse run. The point of memory is to avoid one.

Reacquisition runs through the same parser layers as a parse, but in
a degenerate mode:

- The full scroll loop is replaced with up to `max-reacquire-steps`
  targeted observations.
- Item parsing runs per observation but only emits the candidates
  whose merged identity matches the target (or all of them, when
  diagnosing why the target was lost).
- Region detection still runs unchanged — the region may have moved
  since memory was written, and reacquisition must re-find it before
  reading the viewport.

## Reacquisition outcome shape

```rust
pub enum ReacquireOutcome {
    Reacquired {
        node: ViewNode,
        strategy_used: ReacquireStrategy,
        confidence: Confidence,
        observations: Vec<ViewObservation>,
        diagnostics: Vec<ParserDiagnostic>,
        artifacts: Vec<ArtifactRef>,
    },
    Stale {
        reason: StaleReason,
        diagnostics: Vec<ParserDiagnostic>,
        artifacts: Vec<ArtifactRef>,
    },
    NotFound {
        attempted_strategies: Vec<ReacquireStrategy>,
        diagnostics: Vec<ParserDiagnostic>,
        artifacts: Vec<ArtifactRef>,
    },
}

pub enum StaleReason {
    MemoryRejectedAtFreshness,    // ViewMemory v0 read returned None
    SchemaMismatch,                // memory was view-memory-v0; reader is later
    BaselineMismatchHard,          // region baseline drifted past hard threshold
    RegionGoneAtReacquisition,     // region no longer present in any attempted observation
}
```

`Reacquired` is the success path; the caller can act on `node`.
`Stale` and `NotFound` are both observed failures — they return
`Ok(...)` from the entry point per the diagnostic policy's
Ok-plus-Fatal pattern.

## The cascade

For a target, run matchers in this order. The first to return a
match wins. If none match, output `NotFound`.

```text
1. ViewNodeId direct match              (when target is a ViewNodeId)
2. AxPath match                          (when memory carries AX path on the anchor)
3. LabelMatch in current viewport       (no scroll)
4. ViewportFingerprintNeighborhood     (scroll to memory's recorded viewport)
5. LabelPlusSectionContext              (scroll, then walk by section)
6. Mixed                                  (heuristic: try label after AX fallback)
```

The cascade is **read-side**. Each step is an `ItemParser` + region
re-run with bounded scroll attempts. The fall-through from one step
to the next records why the previous failed, so a `NotFound` carries
the full attempted-strategy history.

Stage ordering is a `REVIEW` item:

> **REVIEW(reacquire-stage-ordering):** v0 puts direct id match first
> because content-derived `ViewNodeId`s should be stable when the
> underlying content has not changed. If real NetEase captures show
> ids drift more than labels (e.g. because section_hint resolution
> changed between runs), demote id match. Track which stage produced
> the eventual match in `view.reacquire.stage_used` span attributes.

### Stage 1: ViewNodeId direct match

Given a target `ViewNodeId`:

1. Run one observation at the current viewport pose (no scroll).
2. Run item parsing, build the per-observation candidates, derive
   their `ViewNodeId`s via the v0 derivation rules.
3. If any candidate's id equals the target, this is the match.

Confidence on match:

- If candidate's `Confidence::level >= Likely` and the matched node
  has at least one OCR + AX evidence ref pair: `Confirmed`.
- Otherwise: copy candidate's confidence.

This stage assumes the target is currently on-screen. If not visible
at the current pose, falls through.

> **REVIEW(direct-id-current-viewport-only):** v0 does not scroll in
> stage 1. If the target id is not in the current viewport, fall
> through to stages 2–6 which include scroll. Trade-off: a single
> scroll attempt here might catch easy cases faster, but it also
> bypasses the principled stage ordering. Measure before changing.

### Stage 2: AxPath match

Activated only if the anchor carries an `AxPath` selector (the
memory writer attached it because the original observation had AX
evidence).

1. Capture the AX tree at the current pose.
2. Walk to the saved AX path. If found and bounds intersect the
   target region, this is the match.
3. Bounds tolerance: same `clip-tolerance` as item parsing
   (`REVIEW(clip-tolerance)`), reused here.

Confidence: `Confirmed` when AX node exact path matches; `Likely`
when path is the closest available ancestor.

### Stage 3: Label match in current viewport

Try item parsing at the current viewport without scrolling. Match
candidates by normalized label equality to the target's saved label.

Match wins when:

- Exactly one candidate matches the label, **or**
- Two or more match but only one matches both label and saved
  `section_hint`.

If multiple match and none are disambiguated by section, emit
`SectionAmbiguous` and fall through.

> **REVIEW(label-stage-multiple-matches-fallthrough):** v0 falls
> through to subsequent stages on ambiguity. If real captures show
> the saved bounds reliably disambiguate, allow stage 3 to pick by
> bounds proximity before falling through.

### Stage 4: ViewportFingerprintNeighborhood

Use the saved `viewport_fingerprint_hint` to scroll to the
fingerprinted position, then run label match:

1. Read the memory's recorded `bounds_window_local` and
   `viewport_fingerprint_hint` for the target node.
2. Issue scroll steps toward the recorded Y range, **bounded** by
   `max-reacquire-scroll-attempts` steps in either direction.
3. After each scroll step, run one observation; if its fingerprint
   matches the recorded one (or hashes within
   `fingerprint-near-match-distance`), run label match.
4. If matched, success.

Thresholds:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `max-reacquire-scroll-attempts` | 5 (in either direction) | `REVIEW(max-reacquire-scroll-attempts)` |
| `fingerprint-near-match-distance` | 0 (exact match only) | `REVIEW(fingerprint-near-match-distance)` |

> **REVIEW(fingerprint-near-match-distance):** v0 only accepts exact
> fingerprint matches because v0 fingerprints are content-hashed and
> "near" is ill-defined. If the fingerprint algorithm later becomes
> perceptual (e.g. average hash), allow a small Hamming distance.

### Stage 5: LabelPlusSectionContext

Targeted scroll plus structured walk:

1. Scroll to the top of the region (per scroll-loop v0
   `scroll_to_top` mechanism).
2. Run observations in the canonical downward direction, but each
   step's `should_stop` triggers as soon as the target section is
   passed.
3. Within the target section, label-match candidates.

Bounded by `max-reacquire-section-walk-steps` and the scroll loop's
own `max_steps` (but this stage gets a tighter budget):

| Constant | v0 default | REVIEW key |
|---|---|---|
| `max-reacquire-section-walk-steps` | 20 | `REVIEW(max-reacquire-section-walk-steps)` |

This is the most expensive stage. It should not be reached on
healthy memory; reaching it means the memory's positional
information was stale enough that fingerprint-based lookup failed.

### Stage 6: Mixed

Fallback that combines available strategies. v0 does:

1. Repeat stage 2 (AxPath) if memory had AX evidence but the path
   walk produced no result earlier — sometimes the AX hierarchy
   reorganizes mid-run.
2. Repeat stage 3 (Label) one more time after letting any animation
   settle (`mixed-settle-delay-ms`).

| Constant | v0 default | REVIEW key |
|---|---|---|
| `mixed-settle-delay-ms` | 250 | `REVIEW(mixed-settle-delay-ms)` |

If stage 6 finds nothing, the outcome is `NotFound`.

> **REVIEW(mixed-stage-existence):** v0 keeps stage 6 as an explicit
> fallback. If empirical data shows stage 6 never finds anything
> stages 1–5 missed, drop it and emit `NotFound` after stage 5.

## Boundary diagnostics

Reacquisition can hit these conditions; each maps onto an existing
diagnostic kind:

| Condition | Diagnostic |
|---|---|
| Target region not present at all | `RegionNotFound` |
| Region present but unparseable | `RegionCollapsed` |
| Reacquisition stuck (scroll did not move) | `ScrollStuck` |
| Modal blocks during reacquisition | `ModalBlocked` |
| Target label is in the reconstruction but in a different section than memory recorded | `SectionAmbiguous` (Warn) — but reacquisition succeeds if the new section is plausible |
| Target label appears partially clipped in every observation | `ItemPartiallyVisible` — reacquisition may still succeed if confidence is `Likely` or better |
| No matching candidate after the cascade | (no policy diagnostic; reflected in `NotFound.attempted_strategies`) |

Adding a new diagnostic kind for reacquisition is forbidden in v0;
all reacquisition failures use the existing kinds.

## Confidence aggregation

`Reacquired.confidence` is computed from:

- The stage that produced the match (id-based > AX > label-based).
- The candidate confidence from item parsing at the matching
  observation.
- A staleness factor based on age of memory vs `memory-hard-ttl`.

The age factor follows the table:

| Age fraction of TTL | Multiplier |
|---|---|
| < 25 % | 1.0 |
| 25–75 % | 0.85 |
| 75–100 % | 0.6 |

If the underlying candidate confidence is `Confirmed`, multiplier
≥ 0.85 keeps the result `Confirmed`; multiplier < 0.85 demotes to
`Likely`.

> **REVIEW(confidence-age-multipliers):** v0's three-band age
> multiplier is a placeholder. If staleness rarely affects match
> quality (i.e. the underlying label stays accurate even when memory
> is old), drop the multiplier. If staleness affects often, switch
> to continuous decay.

## Bounded budget

A single reacquisition call must not exceed:

| Budget | v0 default | REVIEW key |
|---|---|---|
| `max-reacquire-scroll-attempts` (total across stages) | 12 | `REVIEW(reacquire-scroll-budget)` |
| Wall-clock budget | 8 seconds | `REVIEW(reacquire-wallclock-budget)` |

If budget is exhausted before any stage succeeds, the outcome is
`NotFound` with an `IncompleteEvidence` diagnostic naming the
exhausted budget.

## Span and trace extension

Reacquisition adds a new top-level span under the host run's root,
parallel to `view.parse.*`:

| Span name | Purpose |
|---|---|
| `view.reacquire.<scope_id>` | root of a reacquisition run |
| `view.reacquire.memory_load` | memory lookup + freshness check |
| `view.reacquire.stage.<n>.<strategy>` | each cascade stage attempt; n is 1..6, strategy is the `ReacquireStrategy` name |
| `view.parse.observe.<index>` | each observation (reused from trace layout) |

The root `view.reacquire.*` span carries these required signals on
the enclosing response:

| Signal | Value |
|---|---|
| `view.reacquire.scope_id` | the scope id |
| `view.reacquire.target_kind` | `"node_id"` / `"anchor"` / `"label"` |
| `view.reacquire.outcome` | `"reacquired"` / `"stale"` / `"not-found"` |
| `view.reacquire.stage_used` | name of the stage that produced the match, or `"none"` |
| `view.reacquire.observation_count` | count as string |
| `view.reacquire.fatal_diagnostic_kind` | only on `not-found` if a Fatal kind fired during reacquisition |

This is a separate signal namespace from `view.parse.*` so callers
that check outcome do not confuse a reacquisition outcome with a
parse outcome.

## Composition with the parser layers

The reacquisition entry point lives in `auv-view::memory::reacquire`
and reuses `RegionParser` and `ItemParser` implementations from the
example crate. v0 does not introduce a new parser layer; the cascade
is wrapper logic around the existing layer traits, bounded by the
budgets above.

```rust
pub fn reacquire<R, I>(
    memory: &ViewMemory,
    target: ReacquireTarget,
    region_parser: &R,
    item_parser: &I,
    adapter: &dyn ReacquireDriverAdapter,
    config: &ReacquireConfig,
) -> AuvResult<ReacquireOutcome>
where
    R: RegionParser,
    I: ItemParser,
{ ... }

pub enum ReacquireTarget {
    NodeId(ViewNodeId),
    Anchor(String), // anchor_id
    LabelWithSection { label: String, section_hint: Option<String> },
}

pub trait ReacquireDriverAdapter {
    fn capture_viewport(&self, /* ... */) -> AuvResult<ViewViewport>;
    fn collect_evidence(&self, /* ... */) -> AuvResult<Vec<ViewEvidenceNode>>;
    fn scroll_step(&self, axis: ScrollAxis, delta: i32) -> AuvResult<ScrollStepResult>;
    fn capture_ax(&self, /* ... */) -> AuvResult<Vec<SurfaceNode>>;
}
```

The adapter trait is the v0 seam between the framework-side
algorithm and the platform-side driver primitives. NetEase ships an
impl in `auv-example-netease-playlist/src/adapters/`.

## v0 done criteria

Reacquisition is v0-complete when:

1. The cascade is implemented in `auv-view::memory::reacquire` with
   the 6 stages in the specified order.
2. `ReacquireOutcome` is returned (never `Err(...)`) for all
   observed failures; `Err` is reserved for infrastructure
   failures.
3. All thresholds live in a single `ReacquireConfig` struct.
4. The diagnostic kinds emitted are exactly those in the table;
   no new diagnostic kind is invented for reacquisition.
5. Span hierarchy under `view.reacquire.*` matches the trace
   extension; 6 root-span signals are present on every call.
6. Bounded budgets (scroll attempts, wall-clock) cap reacquisition
   regardless of cascade depth.
7. NetEase example exercises stages 1, 3, 4, and 5 at minimum via
   recorded fixtures; stages 2 and 6 are exercised when AX evidence
   is available.

## Forbidden in v0

- Performing a full parse loop inside reacquisition. The whole
  point is to avoid one.
- Returning `Err(...)` for stale memory or failed match. Both
  return `Ok(Stale)` or `Ok(NotFound)`.
- Adding a 7th cascade stage without revising this spec.
- Reading memory without applying the ViewMemory v0 freshness
  rules. Reacquisition does not bypass staleness.
- Promoting a `Likely` confidence match to `Confirmed` when the
  age multiplier would demote it. Multiplier is one-directional
  (demote only).
- Mutating the memory during reacquisition. Reacquisition is
  read-only against memory; updates happen only on the next parse
  run.
- Inventing a parallel `ReacquireDiagnostic` enum. Use existing
  `ParserDiagnostic` kinds.

## Non-goals for this spec

Intentionally deferred:

- Multi-target reacquisition in a single call. v0 takes one target.
- Cross-app reacquisition (target observed in app A, found in app
  B). v0 is single-app.
- Predictive prefetch (warming the next likely target).
- Cache of recent reacquisition outcomes.
- User-facing rendering of reacquisition diagnostics — the entry
  point returns `ReacquireOutcome`; CLIs render per their own
  rules.
- Performance budgets beyond the wall-clock cap.
- Reacquisition under modal dismissal (i.e. dismissing the modal
  to continue). v0 surfaces `ModalBlocked` and stops.

## How to use this spec

When implementing or tuning reacquisition:

- All thresholds in `ReacquireConfig`.
- The 6-stage cascade is closed for v0; if a real case needs a 7th
  stage, file a gap before adding.
- Every `REVIEW(...)` marker is a known incomplete decision. Treat
  numbers as starting points; record measured cache-hit rate /
  stage-usage histogram / wall-clock distribution before tuning.
- Wire `view.reacquire.*` spans / signals exactly as specified.
  Readers depend on them to distinguish parse from reacquisition in
  the same trace.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
