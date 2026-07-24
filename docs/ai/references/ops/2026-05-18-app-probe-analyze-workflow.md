# AUV App Probe and Analyze Workflow v0

Date: 2026-05-18

Status: active reference for `probe -> analyze` only

## Purpose

This document records the current app-surface workflow that still exists on
`main`: `auv-cli app probe` captures an app probe directory, and
`auv-cli app analyze` turns that directory into `analysis.json` plus a human
report.

The old `app distill` / `app validate` lane is no longer active CLI behavior.
Current parsing rejects both subcommands with:

`app recipe distillation has been removed; use app-local Rust commands instead`

That means this workflow is no longer `probe -> analyze -> distill -> validate`.
The live contract stops at analysis.

## CLI Entry Points

Current live entry points:

- `auv-cli app probe <bundle-id> [--output-dir <dir>]`
- `auv-cli app analyze <probe-dir-or-probe-json>`

Current removed entry points:

- `auv-cli app distill ...`
- `auv-cli app validate ...`

Those removed routes are kept only as parser errors, plus a regression test
that locks the hard-error behavior.

## Probe Output

`app probe` writes one probe directory containing:

- `probe.json`

The probe records app identity plus a sequence of invoke-backed probe steps.
The current step ids and command ids are:

1. `probe-permissions` -> `app.probePermissions`
2. `list-displays` -> `display.list`
3. `activate-target-app` -> `app.activate` (only when the app is AppleScript-addressable)
4. `list-windows` -> `window.list`
5. `capture-ax-tree` -> `window.captureAxTree`
6. `capture-window` -> `window.capture`
7. `ocr-sample` -> `window.observeRegion`

Each recorded step currently stores:

- `id`
- `command_id`
- `target_application_id`
- exact `inputs`
- `run_id`
- `span_id`
- `status`
- `output_summary`
- legacy `artifact_paths`
- structured `artifacts`
- optional `failure_message`

Important truth boundary:

- `app probe` intentionally allows several target-specific steps to fail
  without aborting the whole probe.
- That is not hidden. Failures are written into `probe.json`.
- `app analyze` is expected to surface those failures as `known_boundaries`,
  not pretend the evidence existed.

This matters because several probe commands still sit on explicit typed-API
gaps in `auv-cli-invoke`:

- `app.activate`
- `window.captureAxTree`
- `window.observeRegion`

When those fail, probe still completes and records the failure.

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

The report intentionally does not emit recipe or case-matrix output anymore.
Current tests assert that the rendered report omits `recipe:` and `case matrix`
language.

`analysis.json` remains the machine-facing artifact. It contains:

- app identity
- window context
- permission state
- surface assessments
- annotation candidates
- known boundaries
- recommended strategies

It is still an evidence summary, not a promotion or validation artifact.

## Current Analysis Behavior

`app analyze` currently degrades gracefully when probe evidence is partial.

If any of these probe readers fail:

- permission state
- display snapshot
- window snapshot
- AX snapshot
- OCR snapshot

analysis does **not** abort by default. Instead it:

- appends a boundary note to `known_boundaries`
- falls back to a default empty/partial snapshot
- continues producing `analysis.json` and `report.md`

This is the current honesty rule for the workflow: prefer explicit
`known_boundaries` plus weaker assessments over inventing a clean semantic
surface from missing data.

## Truth Boundaries

`app analyze` is not a validator.

It can recommend candidate strategies, but it must not silently promote them to
validated skills or pretend recipe generation still exists. Its output is
bounded by:

- `probe.json`
- whatever artifacts the recorded probe steps actually produced
- current invoke command behavior
- current analysis heuristics

It should prefer:

- `candidate`
- `partial`
- `likely`
- `unknown`

over false certainty.

## What This Workflow Does Not Prove

This workflow does not prove:

- semantic success
- validated end-to-end app actions
- full skill stability
- cross-app reuse
- cross-platform reuse
- any active `distill` / `validate` pipeline

It only establishes a probe-backed app-surface baseline and an honest analysis
summary of that baseline.

## Historical Note

Earlier versions of this document described active `app distill` and
`app validate` outputs. Those sections were accurate for an earlier lane but
they are stale against current `main` and should not be read as approval to
restore that pipeline.
