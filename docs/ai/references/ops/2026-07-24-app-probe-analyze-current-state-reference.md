# App Probe / Analyze Current State Reference

Date: 2026-07-24

Status: active reference

## Purpose

This note replaces the old four-stage `probe -> analyze -> distill -> validate`
workflow as the current repo truth.

As of `main` at `a41f4c29`, the supported app-surface lane is:

`probe -> analyze`

The old distillation and validation stages were removed from the CLI surface and
should be read as historical design material, not current behavior.

## Live CLI Truth

The current root CLI help advertises only:

- `auv app probe <bundle-id> [--output-dir <dir>]`
- `auv app analyze <probe-dir-or-probe-json>`

See:

- `crates/auv-cli/src/cli.rs` `help_text()`
- `crates/auv-cli/src/cli.rs` `parse_app()`

`parse_app()` now hard-errors on the removed subcommands:

- `app distill`
- `app validate`

with:

`app recipe distillation has been removed; use app-local Rust commands instead`

That removal is also locked by `parse_app_distill_and_validate_are_removed()`
in the same file.

## Current Runtime Boundary

The current app module describes itself as:

`App-centric workflows: probe -> analyze.`

See `src/app/mod.rs`.

This boundary is important:

- `app probe` records deterministic app-surface evidence into `probe.json`
- `app analyze` consumes that probe and writes:
  - `analysis.json`
  - `report.md`
- the lane is review/evidence oriented, not recipe promotion

## Current Probe Step Shape

`probe_app_into_run()` in `src/app/mod.rs` currently records these steps:

1. `probe-permissions` -> `app.probePermissions`
2. `list-displays` -> `display.list`
3. `activate-target-app` -> `app.activate` when AppleScript activation is available
4. `list-windows` -> `window.list`
5. `capture-ax-tree` -> `window.captureAxTree`
6. `capture-window` -> `window.capture`
7. `ocr-sample` -> `window.observeRegion`

The exact set is probe-scoped and may record partial failure instead of
aborting the entire probe.

## Current Analyze Boundary

`app analyze` remains a deterministic read-side step over `probe.json`, not a
validator or promotion engine.

The important honesty boundary is still:

- analyze may emit reviewable candidates and known limits
- analyze does not promote those surfaces into action-grade runtime truth

The code boundary for that lane lives in:

- `src/app/analysis.rs`
- `src/app/report.rs`

## Historical Note

The following older ops notes still describe the retired four-stage workflow and
should be treated as historical phase-2 design material:

- `2026-05-18-app-probe-analyze-workflow.md`
- `2026-05-19-v2-docs-contract.md`

Keep them for design history, but do not cite them as the current CLI contract.
