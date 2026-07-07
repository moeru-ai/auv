# Archived Candidate-Action Removal Spec

**Date:** 2026-07-07  
**Status:** proposed  
**Change class:** narrow refactor / archived vertical removal  
**Owner intent:** remove the unused archived `candidate-action` execution path and its special MCP/CLI entrypoints.

## Problem

`candidate-action` was useful as a macOS AX vertical proof, but it is no longer
part of the active AUV product lane. The repository still exposes it through:

- CLI: `candidate-action run`
- MCP: `candidate_action_run`
- Runtime: `Runtime::run_candidate_action_command`
- Command implementation: `src/candidate_action_command.rs`
- Supporting artifacts and read-side lineage for candidate-action decision,
  execution, and promotion artifacts

This creates two problems:

1. MCP carries a product-specific archived command even though MCP should remain
   a generic frontend over current runtime capabilities.
2. Root-level code still contains AX recognition and candidate-action execution
   adapters whose main consumer is the archived vertical.

The cleanup target is the unused execution path, not a broad rewrite of the
current runtime, driver, or view-parser architecture.

## Goals

- Remove the archived `candidate-action` execution entrypoints from CLI, MCP,
  and `Runtime`.
- Delete the command implementation and direct call chain that exists only to
  run candidate-action.
- Remove read-side and inspect surfaces that only present candidate-action
  decision/execution/promotion lineage.
- Delete tests that only validate the removed archived path.
- Keep MCP focused on generic current surfaces such as run inspection and
  runtime invocation.

## Non-Goals

- Do not add a replacement candidate-action command.
- Do not preserve compatibility for private historical candidate-action run
  artifacts.
- Do not expand macOS AX behavior while deleting this path.
- Do not migrate `candidate-action` behavior into `auv-view`.
- Do not delete reusable core seams only because they were once exercised by
  candidate-action.

## Scope

### Delete

Delete or remove references to:

- `src/candidate_action_command.rs`
- `candidate-action run` parsing and dispatch in `src/cli.rs`
- `candidate_action_run` tool, request shape, parser helpers, and tests in
  `src/mcp.rs`
- `Runtime::run_candidate_action_command` in `src/runtime.rs`
- Candidate-action decision/execution artifact production if no remaining
  production caller exists
- Candidate-action and candidate-promotion lineage extraction/rendering paths
  that exist only to read removed archived artifacts
- Tests and fixtures that construct only removed candidate-action artifacts

### Re-evaluate After Deleting Callers

After the entrypoints and command layer are gone, run symbol searches before
deciding final file deletion:

- `src/candidate_action_decision.rs`
- `src/ax_recognition.rs`
- `src/candidate_promotion_recording.rs`
- `src/action_resolver_decision.rs`

If a file has no production consumer after the archived path is removed, delete
it and its tests. If a type is still used by active runtime/read-side code,
either keep it or move it in a follow-up slice with a narrower design.

### Keep

Keep `src/candidate_promotion.rs` in this slice.

Reason: the repository guide identifies `candidate_promotion` as a reusable
promotion/gating seam, not a `candidate-action`-private module. Removing it
requires a separate owner-approved decision that explicitly retires the
`RecognitionResult -> Candidate` promotion concept.

## Expected Data Flow After Removal

MCP should no longer have a special archived command path:

```text
MCP
  -> generic current runtime / inspect APIs
```

The removed path is:

```text
MCP candidate_action_run
  -> Runtime::run_candidate_action_command
  -> candidate_action_command
  -> ax_recognition
  -> candidate_promotion_recording
  -> candidate_action_decision
  -> macOS driver input
```

CLI should likewise stop advertising or parsing `candidate-action run`.

## Read-Side Decision

Do not keep candidate-action artifact compatibility in this slice.

Rationale:

- The project is not public.
- The archived path is not part of the current product lane.
- Keeping compatibility would preserve large read-side branches for artifacts
  that new code can no longer produce.

If historical artifact inspection becomes necessary later, it should be handled
as an explicit archive-reader task, not kept in the default current inspect
surface.

## Implementation Notes

Recommended order:

1. Remove MCP `candidate_action_run` and its request/parser tests.
2. Remove CLI `candidate-action run` parsing, dispatch, help text, and tests.
3. Remove `Runtime::run_candidate_action_command`.
4. Delete `src/candidate_action_command.rs` and fix module declarations.
5. Remove candidate-action read-side lineage from `run_read`, `inspect`, and
   `inspect_server`.
6. Run `rg` for removed symbols and delete now-unreferenced support files.
7. Keep `candidate_promotion.rs`; add no new behavior to it.

## Validation

Run:

```sh
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- invoke --help
```

Expected result:

- `candidate-action run` is no longer present.
- MCP no longer exposes `candidate_action_run`.
- Current invoke, run recording, inspect, and app command surfaces still build.
- No production references remain to deleted archived modules.
