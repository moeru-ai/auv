# AUV App Probe and Analyze Workflow

Date: 2026-05-18

Status: active reference, updated for current CLI surface on 2026-07-24

## Purpose

This note describes the current `auv app` workflow that still exists on
`main`.

Today that workflow is:

```text
probe -> analyze
```

The older `distill -> validate` tail documented in May 2026 was removed from
the CLI. This file now records the current command surface and the honesty
boundary that still matters for `app analyze`.

## Current CLI Entry Points

- `auv app probe <bundle-id> [--output-dir <dir>]`
- `auv app analyze <probe-dir-or-probe-json>`

Current parser/runtime truth:

- `crates/auv-cli/src/cli.rs` only accepts `probe` and `analyze` under
  `auv app`.
- `distill` and `validate` now hard-error with:
  `app recipe distillation has been removed; use app-local Rust commands instead`

## Probe Output

`app probe` writes one probe directory containing:

- `probe.json`

The probe records app identity plus these invoke-backed steps:

1. `probe-permissions` -> `app.probePermissions`
2. `list-displays` -> `display.list`
3. `activate-target-app` -> `app.activate`, when AppleScript-addressable
4. `list-windows` -> `window.list`
5. `capture-ax-tree` -> `window.captureAxTree`
6. `capture-window` -> `window.capture`
7. `ocr-sample` -> `window.observeRegion`

Each step preserves its command inputs, target application, run/span identity,
status, output summary, artifacts, and optional failure message.

On the `a41f4c29` baseline, `resolve_probe_ocr_sample_query` still looks up the
legacy step ids `observe-windows` and `observe-window-tree`. The producer ids
above are canonical, but that analyzer fallback is not yet migrated and may
fall back to the app name or bundle id instead.

Important current behavior:

- partial app identity is allowed when LaunchServices or Spotlight cannot fully
  resolve the bundle id
- target-specific failures are allowed to survive inside `probe.json`
- missing or failed probe steps are analysis boundaries, not permission to
  fabricate a cleaner story later

## Analyze Output

`app analyze` consumes `probe.json` and writes:

- `analysis.json`
- `report.md`

The current report/output shape is still review-oriented. It covers:

1. app basic information
2. available surfaces
3. grounding assessment
4. control assessment
5. verification assessment
6. known boundaries
7. recommended strategies
8. surface candidates and candidate-query evidence where available

`analysis.json` remains the machine-readable output. `report.md` remains the
human-facing summary.

## Current Honesty Boundary

`app analyze` is not a validator and does not emit a promoted
`contract::Candidate`. It does carry review-time `promotion_gate` metadata,
including `action_grade_candidate` classifications for the currently supported
families.

It may:

- classify observable surfaces from probe artifacts
- emit reviewable surface candidates
- attach candidate queries, evidence refs, and known limits
- report the analyzer's current promotion classification
- recommend only strategies that current runtime/action contracts can actually
  express

It must not:

- silently turn weak OCR or row evidence into semantic success claims
- treat a classification as proof that a runtime action executed or verified
  semantic success
- hide missing probe truth behind prose-only optimism

When probe truth is weak, the correct output is still:

- zero candidates
- zero recommended strategies
- explicit known boundaries

That is more useful than fake genericity.

## Historical Boundary

One May 2026 note remains as historical contract context:

- [`2026-05-28-surface-analyze-v0.md`](2026-05-28-surface-analyze-v0.md)

Use it for the historical `surface analyze` candidate boundary:

- what `AppSurfaceCandidate` was allowed to mean
- why surface candidates were kept separate from `contract::Candidate`
- what the old analyze-only promotion gate closed

Do not use that note as proof that `auv app distill` or `auv app validate`
still exist on the current CLI. They do not.

## Related Current Code

- [`crates/auv-cli/src/cli.rs`](../../../../crates/auv-cli/src/cli.rs)
- [`src/app/mod.rs`](../../../../src/app/mod.rs)
- [`src/app/analysis.rs`](../../../../src/app/analysis.rs)
