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

It only establishes a probe-backed app-surface analysis baseline that later
`distill` and `validate` steps can consume.

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
