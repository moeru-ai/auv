  # AUV App Probe and Analyze Workflow v0

Date: 2026-05-18

Status: active reference

## Purpose

This workflow is the current phase-2 entrypoint for `probe -> analyze -> distill -> validate`.

It exists to stop the next distillation loop from starting with free-form model
opinions. The `analyze` step must be grounded in deterministic probe artifacts.

## CLI Entry Points

- `auv-cli app probe <bundle-id> [--output-dir <dir>]`
- `auv-cli app analyze <probe-dir-or-probe-json>`
- `auv-cli app distill <analysis-dir-or-analysis-json> [--output-dir <dir>]`
- `auv-cli app validate <distill-dir-or-distillation-json>`

## Probe Output

`app probe` writes one probe directory containing:

- `probe.json`

The current implementation records:

1. app identity
  - bundle id
  - app name
  - app path
   - main executable path
   - version and build version
   - URL schemes
   - AppleScript addressability

2. deterministic runtime-backed probe steps
   - `debug.probePermissions`
   - `debug.listDisplays`
   - `debug.probeCoordinateReadiness`
   - `debug.activateApp`
   - `debug.observeWindows`
   - `debug.observeAxTree`
   - `debug.captureDisplay`
   - `debug.findImageText` as a sample OCR-on-artifact pass

Each recorded step includes:

- command id
- target application id
- exact inputs
- run id
- output summary
- artifact paths
- inspect path

This means distillation can start from actual runtime traces instead of chat
memory.

Important:

- `app probe` is now allowed to capture a **partial** app identity when
  LaunchServices or Spotlight cannot resolve the bundle id to an installed app
  bundle.
- target-specific probe steps such as AX tree observation or app-targeted
  capture are also allowed to fail without aborting the whole probe
  directory.
- those failures are recorded into `probe.json` and must later surface as
  analysis boundaries rather than being silently ignored.

## Analyze Output

`app analyze` consumes `probe.json` and writes:

- `analysis.json`
- `report.md`

The current report shape covers:

1. app basic information
2. available surfaces
3. grounding assessment
4. candidate / annotation layer
5. control strategy
6. verification assessment
7. known boundaries
8. recommended candidate strategies

The structured `analysis.json` is the machine-facing handoff to later
distillation. The Markdown report is for humans and LLM review.

The current `analysis.json` also carries structured candidate annotations rather
than only prose. These candidates are the first machine-consumable layer for
list-like UI targets and ambiguous grounding:

- AX focus-query candidates
- OCR anchor-text candidates
- grouped visible-row candidates when the sampled surface looks collection-like
- primary-window region candidates

Candidate objects should now be read as small target specs, not just labels.
At minimum they may carry:

- `coordinate_space`
- `bounds`
- `click_point`
- `input_bindings`
- `compatibility.direct_taxonomy_ids`
- `compatibility.context_taxonomy_ids`

This is the start of a contract that can later describe fixed-layout or
window-relative action targets without collapsing back into ad-hoc README prose.

The important distinction is:

- direct taxonomy ids mean the candidate can already project concrete recipe
  inputs for that taxonomy
- context taxonomy ids mean the candidate is still useful evidence, but not yet
  a direct recipe-input source

When the semantic surface is weak but the probe still exposed a stable primary
window region, `app analyze` may now emit a `window-primary-region` annotation
derived from either the visible window snapshot or the AX root window fallback.

If the probe captured only a partial app identity or some target-specific
steps failed, `app analyze` should still produce `analysis.json` and `report.md`
as long as enough deterministic baseline facts remain to speak honestly. In
that situation the output should prefer:

- zero candidates
- zero recommended strategies
- explicit `known_boundaries`

over manufacturing candidate slices from missing evidence.

## Distill Output

`app distill` consumes `analysis.json` and writes:

- `distillation.json`
- `report.md`
- `candidates/*.recipe.json`
- `candidates/*.cases.json`

The current distill step is intentionally narrow:

- it generates candidate recipe and case-matrix scaffolds
- it carries forward suggested annotation ids from the source analysis
- it now also records a machine-readable `candidate_shape` per distilled
  candidate:
  - direct candidate ids
  - context candidate ids
  - provided inputs
  - shape notes
- it validates those generated artifacts against the current skill validators
- it does **not** promote them to validated skills

This means `distill` is allowed to produce useful candidate shapes, but not to
invent success claims.

## Validate Output

`app validate` consumes `distillation.json` and writes:

- `validation.json`
- `validation-report.md`

The current validate step is also intentionally narrow:

- it loads candidate recipe/case-matrix pairs from the distillation output
- it first applies any `candidate_shape.provided_inputs` from distill
- it then applies only conservative auto-grounding from the structured analysis candidates
- it classifies each candidate as:
  - `validated`
  - `candidate`
  - `rejected`
- it does **not** promote validated candidates into the main skill tree

The current honesty rule is:

- unresolved `TODO_*` inputs or missing grounding inputs move the candidate to
  `rejected` before execution
- live runtime failures move a runnable candidate to `rejected`
- only successful live execution moves a candidate to `validated`

This is deliberately stricter than the distillation phase. `app distill` may
emit candidate scaffolds, but `app validate` must not preserve an unresolved
candidate as if validation had made progress. It writes the validation report
with the unresolved inputs and fails the validation command.

## Truth Boundaries

`app analyze` is not a validator.

It can recommend candidate strategies, but it must not silently promote them to
validated skills. Its output is bounded by:

- probe artifacts
- current runtime contracts
- current strategy taxonomy

It should prefer:

- `candidate`
- `partial`
- `likely`
- `unknown`

over false certainty.

## What This Workflow Does Not Prove

This workflow does not prove:

- semantic success
- full skill stability
- cross-app reuse
- cross-platform reuse

It only establishes a probe-backed app-surface baseline that later `distill`
and `validate` steps can consume.

## First Smoke Result

The first live smoke target was `com.apple.TextEdit`.

That smoke run showed the intended behavior:

- `search-entry.ax-text-input.clipboard-submit.capture-evidence`
- `native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text`

were emitted as candidate strategies.

It intentionally did **not** emit a bogus `result-selection` candidate just
because a sample OCR query matched some visible text.

This is the current honesty bar for `app analyze`: avoid over-claiming generic
skill shapes that the sampled app surface does not justify.

The same honesty bar now applies to `app distill`: candidate files must be
machine-valid and strategy-consistent, but they must stay clearly marked as
candidate-only until the validate/promote path proves them live.

The same honesty bar now applies to `app validate`: auto-grounding can help,
but it must stay conservative. If validate cannot resolve a `focus_query`,
`anchor_text`, or similar candidate input honestly, it must reject that
candidate before execution rather than manufacturing a fake validated result.

The current honesty bar also applies to the candidate layer itself: a noisy OCR
match should still be emitted as a noisy OCR candidate instead of being silently
rewritten into a cleaner but false anchor.

The same rule now applies one step earlier to `app probe`: missing app identity
resolution, failed AX capture, or failed app-targeted screenshot should survive
as recorded probe truth. They are boundaries, not excuses to abort the entire
workflow before analysis can describe the problem.

The OCR sample inside `app probe` also now prefers observable window/app labels
over metadata-only names when those labels are available from the live surface.
This matters for localized desktop apps where:

- `app.app_name` may come back as an English bundle-facing name
- the visible UI may expose a different localized app name
- blindly probing OCR with metadata can create a false-zero sample

That change improves probe honesty because the sample query is now closer to
the text a human would actually expect to find on the captured window.

## Second Smoke Result

The current `TextEdit` smoke now covers the full phase-2 chain:

- `app probe`
- `app analyze`
- `app distill`
- `app validate`

That smoke produced two candidate outcomes:

- `macos.textedit.native_text_candidate.v0` -> `validated`
  - validate reused a live AX text-surface query (`First Text View`)
  - the marker paste completed
  - `debug.verifyAxText` verified the same marker through AX

- `macos.textedit.search_entry_candidate.v0` -> `rejected`
  - validate refused to invent a fake `focus_query`
  - it only auto-filled the trivial `query`
  - the candidate therefore failed before execution with explicit unresolved
    grounding inputs

This is the current honesty bar for `app validate`:

- promote only the slices that really run live
- reject unresolved candidate slices before execution
- do not use auto-grounding as permission to fabricate validation

## Fixed-Layout Pointer Result

The current NetEaseMusic V2 pass now covers a different kind of truth:

- `app probe` now recovers one localized OCR sample query (`网易云音乐`) instead
  of blindly reusing the English metadata name (`NeteaseMusic`)
- that sample now yields visible OCR anchors, but only at the app-title layer
- `app analyze` can recover one `window-primary-region` candidate from the AX
  root window even when `observe-windows` reports zero visible windows
- `app distill` can emit one generic
  `window-action.window-point.pointer-click.capture-evidence` candidate
- that candidate now carries machine-readable `window_bounds`, `relative_x`,
  and `relative_y` bindings derived from the primary window region
- `app validate` can conservatively auto-ground `relative_x` and `relative_y`
  from that same annotation and run the slice live
- the current live NetEaseMusic smoke therefore validates one
  `window-action.window-point.pointer-click.capture-evidence` slice through:
  - `debug.activateApp`
  - `debug.clickWindowPoint`
  - `debug.captureWindow`

That is the current honesty bar for fixed-layout baselines:

- keep the window-relative pointer slice machine-readable
- allow activation-level pointer slices to validate when the analysis really
  carries enough grounding data
- keep weak OCR-title anchors as weak OCR-title anchors instead of inflating
  them into result-selection truth
- do not pretend that this is already a semantic search, result-selection, or
  playback skill

## Relationship To V2

This workflow is the execution spine for V2, not the full V2 scope by itself.

See also:

- `docs/ai/references/ops/2026-05-19-v2-docs-contract.md`

That note defines:

- what V2 is allowed to focus on
- what V2 must not reopen
- why `xcap` capture should now be consumed as current truth instead of
  redesigned again inside this phase
