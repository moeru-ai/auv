# AUV Repo State Closure — 2026-05-21

Status: closed-loop repo audit

## Purpose

This note records the **actual repository state** after multiple parallel
implementation lines (overlay, inspect viewer, and live inspect recording)
started to drift across branches, worktrees, and handoff docs.

It is intentionally narrow:

- no new product direction
- no new runtime behavior
- no new viewer features
- only branch truth, doc truth, and next-step closure

## Main Branch Truth

Current `main` head during this audit:

- `1a501a8` — `Merge pull request #4 from moeru-ai/feature/live-inspect-recording`

This means:

- PR #4 is already merged.
- The inspect-server live recording substrate is on `main`.
- The viewer implementation commits referenced by the design handoff are also
  reachable from `main`.

## Viewer Truth

The inspect viewer work claimed in the design handoff is **present on `main`**.

Confirmed markers in `main`:

- `Events · events.jsonl` exists in
  `src/inspect_server_viewer.html`
- `Artifacts · /artifacts` exists in
  `src/inspect_server_viewer.html`
- WebSocket live-stream endpoint strings (`/runs/:id/stream`) exist in the
  viewer payload
- inspect-server tests include payload marker coverage for:
  - events rail
  - artifact panel
  - live stream wiring

Viewer payload size during this audit:

- `src/inspect_server_viewer.html` = `46582` bytes

This is large but still self-contained. The earlier C.5 asset-route pivot
already happened and is part of `main`.

## Documentation Truth

Before this closure pass, the design docs disagreed:

- `docs/design/IMPLEMENTATION_HANDOFF.md` said C.3a/C.3b/C.4/C.5 were shipped
- `docs/design/README.md` still said those phases were pending

That contradiction was the main repo-state drift affecting agent handoff.

This closure pass updates `docs/design/README.md` so it matches the shipped
viewer state described in `IMPLEMENTATION_HANDOFF.md`.

## Worktree Truth

Active worktrees during this audit:

- `/Users/liuziheng/https-github-com-moeru-ai-auv` → `main`
- `/private/tmp/auv-pr4-review` → `codex/review-pr4`
- `/Users/liuziheng/https-github-com-moeru-ai-auv-macos-pr` → `codex/auv-macos-capability-pr`
- `/Users/liuziheng/https-github-com-moeru-ai-auv-observation-pr` → `codex/auv-observation-first-pr`
- `/Users/liuziheng/https-github-com-moeru-ai-auv-runtime-pr` → `codex/auv-runtime-pr`

Branch reality:

- `codex/review-pr4` is no longer an active product branch; its work is merged.
- `codex/auv-macos-capability-pr`, `codex/auv-observation-first-pr`, and
  `codex/auv-runtime-pr` are all significantly behind `origin/main` and should
  be treated as stale until explicitly rebased or retired.

This matters because the existence of those worktrees no longer implies they
are the right place to continue work.

## What Is Closed

As of this note, the following lines should be treated as **closed enough to
stop pushing blindly**:

- PR #4 live inspect recording substrate
- viewer shell + run list
- viewer span tree
- viewer events rail
- viewer artifact panel
- viewer WebSocket live stream
- viewer `/assets/:name` route

Closed does **not** mean “perfect”; it means the next step should be
**evaluation or consolidation**, not more speculative feature layering.

## What Is Not Closed

These remain open, but should not be tackled by default without a fresh
decision:

- screenshot-first inspect viewer experience
- cross-app `smartPress` matrix (`Phase 3 #5`)
- QQ音乐 row-fallback re-grounding on `smartPress` (`Phase 3 #6`)
- broader multi-app disturbance validation

Those are product/validation lines, not infrastructure closure lines.

## Recommended Next Step

Do **not** continue Phase 3 by inertia.

The next high-value step is:

1. run the shipped viewer for real
2. inspect the visual behavior with an actual browser session
3. decide whether the next move is:
   - viewer polish,
   - screenshot-first redesign,
   - or a real `smartPress` validation experiment

In short:

> stop building from chat momentum and switch to evaluation from live behavior

