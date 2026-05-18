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
   - `debug.probeDisplays`
   - `debug.probeCoordinateReadiness`
   - `debug.observeWindows`
   - `debug.observeWindowTree`
   - `debug.captureScreen`
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

## Analyze Output

`app analyze` consumes `probe.json` and writes:

- `analysis.json`
- `report.md`

The current report shape covers:

1. app basic information
2. available surfaces
3. grounding assessment
4. control strategy
5. verification assessment
6. known boundaries
7. recommended candidate strategies

The structured `analysis.json` is the machine-facing handoff to later
distillation. The Markdown report is for humans and LLM review.

## Distill Output

`app distill` consumes `analysis.json` and writes:

- `distillation.json`
- `report.md`
- `candidates/*.recipe.json`
- `candidates/*.cases.json`

The current distill step is intentionally narrow:

- it generates candidate recipe and case-matrix scaffolds
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
- it applies only conservative auto-grounding
- it classifies each candidate as:
  - `validated`
  - `candidate`
  - `rejected`
- it does **not** promote validated candidates into the main skill tree

The current honesty rule is:

- unresolved `TODO_*` inputs keep a candidate in `candidate`
- live runtime failures move a runnable candidate to `rejected`
- only successful live execution moves a candidate to `validated`

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

- `search-entry.ax-text-input.clipboard-submit.capture-screen-evidence`
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
`anchor_text`, or similar candidate input honestly, it must leave the skill in
`candidate` rather than manufacturing a fake validated result.

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

- `macos.textedit.search_entry_candidate.v0` -> `candidate`
  - validate refused to invent a fake `focus_query`
  - it only auto-filled the trivial `query`
  - the candidate therefore remained runnable-in-principle but unresolved

This is the current honesty bar for `app validate`:

- promote only the slices that really run live
- keep unresolved candidate slices in `candidate`
- do not use auto-grounding as permission to fabricate validation
