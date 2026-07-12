# AUV V2 Docs Contract

Date: 2026-05-19

Status: working phase-2 contract

## Purpose

This note defines what "V2" means after:

- phase 1 freeze
- the current `probe -> analyze -> distill -> validate` workflow landing
- the macOS capture backend switching to `xcap`

The goal is to stop phase 2 from turning into another vague exploration loop.
V2 needs a clear scope, clear inputs, and clear non-goals.

## Current Code Truth

As of `origin/main` at `ed4a93e`, the current capture surface is already
renamed and implemented through `xcap`:

- `debug.captureDisplay`
- `debug.captureRegion`
- `debug.captureWindow`
- `debug.listDisplays`

Important:

- `debug.captureScreen` is **not** currently present in the command catalog.
- `debug.probeDisplays` is **not** currently present in the command catalog.

If a legacy alias is desired later, that is a compatibility discussion. It is
not the current repo truth.

V2 should therefore consume the current `xcap`-based capture surface instead of
reopening the naming or migration argument inside this phase.

## V2 Objective

V2 is the phase where AUV stops being only a collection of validated narrow
skills and starts becoming a repeatable app-surface distillation workflow.

The target workflow is:

`probe -> analyze -> distill -> validate -> promote`

The product goal is:

- analyze one app surface from deterministic probe artifacts
- generate candidate recipes and case matrices
- validate those candidates through the shared runtime
- promote only the validated slices into the skill tree or bundle product

## V2 Scope

V2 should focus on five concrete contracts.

### 1. App-Surface Analysis Contract

`app analyze` must keep producing:

- machine-consumable `analysis.json`
- human-readable `report.md`
- structured candidate / annotation objects derived from probe artifacts

This contract must also tolerate partial probe truth:

- unresolved app identity
- failed target-specific AX steps
- failed target-specific capture steps

As long as the probe still recorded deterministic baseline facts, `app analyze`
should continue and convert those failures into explicit boundaries instead of
crashing the whole lane.

The report should describe:

- app identity
- available surfaces
- grounding assessment
- control assessment
- verification assessment
- known boundaries
- recommended strategies

But it must not silently upgrade recommendation into validation truth.

The candidate / annotation layer is where V2 starts turning prose into
machine-consumable grounding hints. It should be able to represent at least:

- AX focus-query candidates
- OCR text-anchor candidates
- visible-row candidates for list-like UI targets
- stable window or region candidates

Those candidates should not only name things. They should also be able to carry
small target-spec fields such as:

- `coordinate_space`
- `bounds`
- `click_point`
- `input_bindings`
- `compatibility.direct_taxonomy_ids`
- `compatibility.context_taxonomy_ids`

That is the minimum needed to keep fixed-layout local baselines machine-readable
without pretending they are already general-purpose semantic skills.

`direct_taxonomy_ids` and `context_taxonomy_ids` should not be collapsed into
one vague list.

- direct means the candidate can already project real recipe inputs for that
  taxonomy
- context means the candidate is still useful evidence, but not yet an honest
  recipe-input source

But when the probe truth is too weak, the correct output is still:

- zero candidates
- zero strategies
- explicit failure boundaries

That is more useful than fake genericity.

### 2. Distillation Contract

`app distill` must keep producing candidate-only artifacts:

- `distillation.json`
- `candidates/*.recipe.json`
- `candidates/*.cases.json`
- `report.md`

Each distilled candidate should also carry a machine-readable `candidate_shape`
so distill is not forced to rediscover everything from prose later. The shape
should be able to record:

- direct candidate ids
- context candidate ids
- provided inputs
- shape notes

The output must be:

- machine-valid
- strategy-consistent
- still clearly candidate-only

This phase is about generating candidate shapes, not inventing success claims.

### 3. Validation And Promotion Contract

`app validate` is where candidate truth becomes differentiated:

- `validated`
- `candidate`
- `rejected`

But `validated` is only the execution-status boundary. It does not by itself
mean the system machine-proved the user-visible outcome. V2 should carry one
more explicit field alongside it:

- `verification_mode = machine-asserted | evidence-only`

Where:

- `machine-asserted` means the recipe ended in a structured verification step
  such as AX text or OCR title assertion
- `evidence-only` means the recipe completed and retained evidence, but human
  review is still required before treating the outcome as behavior truth

V2 should keep that honesty boundary and extend it into promotion:

- only validated slices are allowed to promote
- candidate slices stay candidate even if they look promising
- rejected slices stay recorded as failure evidence, not silently rewritten

Validate should prefer `candidate_shape.provided_inputs` from distill before it
falls back to analysis-side auto-grounding. Otherwise distill is only pretending
to be candidate-aware while validate still behaves like a prose reader.

Under the current implementation, unresolved recipe inputs are a validation
failure, not a soft candidate outcome:

- `app distill` may emit candidate scaffolds with unresolved `TODO_*` fields
- `app validate` must resolve those fields through `candidate_shape` or honest
  analysis-side grounding before it attempts live execution
- if the fields remain unresolved, the candidate is marked `rejected`, the
  unresolved inputs are written into `validation.json`, and validation returns a
  failure

This keeps "candidate" as a distillation/promotability concept, not a way to
hide that validation could not execute the recipe.

### 4. Target Resolution Contract

V2 should explicitly recognize that app identity and window identity are not
the same thing.

The next execution-side contract should be built around:

- `AppSelector`
- `ResolvedAppRef`
- `WindowRef`

This is the next productization seam for app control.

The problem V2 needs to solve is:

- app selection should not succeed at the bundle-id layer
- then degrade into string heuristics at the window layer

Target resolution has to stay coherent from:

- app identity
- to foregrounding
- to window selection
- to action execution

### 5. Verification Provider Contract

V2 should formalize verification as a provider hierarchy instead of treating
OCR as the only truth source.

Current useful verification families already visible in the repo are:

- image evidence verification
- AX text verification
- AX now-playing verification

V2 should make it clearer which verification surface a skill is using, and
which level of truth it supports:

- activation-only success
- semantic-selection success
- state verification success

## V2 Sample Set

V2 should still keep the validated sample base narrow.

Validated base:

- QQ音乐
- Notes
- TextEdit

Candidate expansion target:

- NetEaseMusic / 网易云音乐

But NetEaseMusic should enter V2 as:

- probe/analyze/distill/validate target

not as a pre-validated product slice.

There is now also a narrow fixed-layout local NetEaseMusic baseline under
`recipes/macos/netease-cloud-music/`.

The current V2 truth for that app is narrower than a playback skill but
stronger than a paper candidate:

- one `window-action.window-point.pointer-click.capture-evidence` slice can now
  validate live from a `window-primary-region` annotation
- that slice is still only an activation-level pointer contract
- it does not yet validate semantic search entry, result selection, or playback
  state truth

That is useful V2 evidence, but it still should not be treated as a promoted
bundle member until the workflow can describe and validate it through the same
truth model used for the existing frozen sample set.

## V2 Non-Goals

The following are explicitly out of scope for V2.

### Do Not Reopen Capture API Design

Do not use V2 to redesign or re-split the capture API again.

Consume:

- `debug.captureDisplay`
- `debug.captureRegion`
- `debug.captureWindow`
- `debug.listDisplays`

Do not turn V2 into another capture naming or migration argument.

### Do Not Reopen Phase 1 OCR Chasing

V2 is not permission to start another endless "one more OCR edge case" loop.

Phase 1 already froze with explicit unresolved boundaries.
V2 should build contracts and promotion logic on top of that truth.

### Do Not Expand To Realtime Tracking

Realtime trace / YOLO / moving-target execution is not V2 work.

That belongs to a later lane because it changes:

- control timing
- state freshness rules
- execution model
- model/provider assumptions

If the team wants:

- async tracking thread
- moving target alignment
- low-latency detector feedback

that should be treated as a separate V3 problem, not quietly smuggled into V2.

### Do Not Widen To Broad Cross-Platform Claims

V2 should not market itself as broad cross-platform reuse.

It should first prove that:

- the distillation workflow is honest
- the strategy contract is stable
- promotion boundaries are real

on the current macOS-native sample set.

## Immediate Next Work

The next V2 steps should be:

1. consume the current `xcap` capture surface without redesigning it
2. tighten selector coherence around `AppSelector / ResolvedAppRef / WindowRef`
3. keep hardening the candidate or annotation layer for list-like UI targets
4. formalize verification-provider truth and semantic-vs-activation boundaries
5. only then promote newly validated slices into the skill tree or bundle layer

## Short Version

V2 is:

- app-surface distillation productization
- contract extraction
- validation and promotion discipline

V2 is not:

- capture API redesign
- another phase-1 OCR spiral
- realtime YOLO tracking
- broad platform expansion
