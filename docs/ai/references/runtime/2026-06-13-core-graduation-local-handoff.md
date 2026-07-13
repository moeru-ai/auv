# Core Graduation Local Handoff

Date: 2026-06-13

Status: local-only next-phase handoff after osu bounded demo closeout; next owner-approved slice is C1 from the core lane roadmap

## What Just Closed

The osu bounded benchmark lane has completed its local approved mission and is now pushed to remote.

Remote commit:

- `fe7ffb4` — `feat(osu): close bounded vision demo lane`

Key local conclusion from the osu lane:

- the AUV-relevant value of the osu work was never “YOLO must become product architecture”
- the lane forced real AUV core questions into the open: scheduler semantics, timestamped capture, frame/action correlation, projection, latency evidence, trace/artifact staging, and offline eval boundaries
- detector / YOLO continuation is better treated as a detector-owner experiment lane, not as required AUV core work

Operational split now intended by the owner:

- `Neko`: YOLO / detector / model-eval continuation if desired
- current owner lane: `core graduation`

## What Must Not Be Re-litigated

Do not reopen these as if undecided:

- osu YOLO is not required for AUV core progress
- bounded P8 closeout is already complete locally and pushed
- detector work beyond the current offline interface is not the current owner lane
- the current owner lane is to graduate truly reusable core semantics, not continue app-specific visual experimentation by inertia

## Core Graduation Goal

Use the evidence forced out by osu and adjacent AUV slices to decide what actually deserves to graduate into core, what should remain app-specific, and what should become a narrower shared helper instead of a broad abstraction.

The working target is not “move more code into core.”
The target is “graduate only the semantics that are proven reusable, auditable, and safe.”

## Highest-Value Candidate Themes

These are the most promising graduation themes based on the completed osu lane and current AUV phase:

1. timestamped capture semantics
2. frame/action correlation keys and lineage
3. latency report / timing evidence reusable shape
4. scheduler clock-start semantics and warm-up boundary
5. runtime / driver boundary rules for input evidence versus semantic verification
6. trace / artifact / lineage semantics that are cross-lane, not osu-specific

These are explicitly **not** graduation targets from the osu lane:

- beatmap parsing
- playfield projection constants
- osu labels / dataset format / play policy
- detector model specifics
- app-specific visual heuristics

## Required Working Method For This Phase

This phase should default to the newly established memory system:

- aggressive parallel research/review is allowed and preferred when uncertainty is high
- subagents are cheap evidence samplers, not scarce budget
- concurrency can expand up to about 15 when useful
- main thread acts as scheduler / integrator / judge
- before major implementation, do first-wave exploration:
  - repo pattern scan
  - ECC / skills / hooks / tooling inventory
  - external docs / prior art when relevant
  - stronger algorithm / architecture alternatives
  - small probes / experiments
  - validation-route planning
- for complex slices, maintain:
  - Exploration Ledger
  - Skill Selection Plan
  - tool / hook / MCP safety and permission checks
  - Graduation Decision Record

## Required Decision Discipline

For any real core graduation slice, the result must end with an explicit Graduation Decision Record, not just code.

Allowed outcomes:

- Graduate to core
- Keep app-specific
- Extract shared helper
- Defer

Each decision must cite:

- stable semantics
- cross-lane reuse evidence
- boundary impact
- safety / consent / verification / trace consequences
- validation evidence
- migration / compatibility impact
- rejected alternatives

## Immediate Next Slice Recommendation

Do not start by writing code.

Start with C1 from `docs/ai/references/runtime/2026-06-13-core-roadmap.md` when the owner says go.

## Suggested First Questions

The next slice should answer questions like:

- which completed lane produced the strongest evidence for a reusable core semantic?
- which candidate has at least two consumers or a clear second-lane justification?
- where is the current boundary too app-specific to graduate safely?
- what can become a shared helper without prematurely becoming “core policy”?
- which candidate can be validated with targeted tests/artifacts instead of broad hope?

## Known Failure Modes To Avoid

- treating “finished implementation” as “approved graduation”
- promoting app-specific shapes because they look elegant
- letting subagents produce overlapping summaries instead of covering distinct uncertainty surfaces
- skipping ECC/tooling/skill inventory and then reinventing available support manually
- doing research-first as ritual procrastination instead of uncertainty compression
- treating a lucky pass as enough for safety-sensitive promotion
- letting convenience outrun isolation for hooks / MCP / automation

## Practical Resume Point

If resuming from compacted context, begin here:

1. read this handoff
2. read `docs/ai/references/runtime/2026-06-13-core-roadmap.md`
3. inspect current memory rules for exploration / arbitration / safety / graduation
4. start `C1` only when the owner says go, using the roadmap's slice boundary and gate
5. stop after the C1 gate and wait for the owner to choose the next slice
