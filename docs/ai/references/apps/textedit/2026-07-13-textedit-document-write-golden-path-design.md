# TextEdit document.write golden path (design)

**Date:** 2026-07-13  
**Status:** accepted for issue [#101](https://github.com/moeru-ai/auv/issues/101)  
**Slice:** owner-approved cross-layer feature

## Objective

Ship one recorded product operation:

```text
app.textedit.document.write
  -> auv-apple-textedit DocumentWrite workflow
  -> auv-driver-macos AccessibilityApi (AX focus + text observe)
  -> auv-cli-invoke invoke_recorded
  -> one canonical run
  -> product CLI / MCP / inspect-server same-run reads
```

## Ownership

| Concern | Owner |
|---|---|
| Workflow (activate → focus → paste → verify) | `auv-apple-textedit` (`DocumentWrite` / `run_document_command`) |
| Typed AX capability | `auv-driver-macos` `AccessibilityApi` on `MacosDriverSession` |
| Core invoke catalog | `auv-cli-invoke::default_registry` (no TextEdit dep) |
| Product registry extension | `auv-cli` (`product_registry` = core + TextEdit) |
| Recording lifecycle | `auv-cli-invoke::invoke_recorded_with_finalize` |
| Inspect composition | existing product `InspectComposer` |

## Public command shape

```text
auv invoke app.textedit.document.write --content <TEXT> [--replace true|false] [--verify true|false] [--target <bundle-id>]
```

Defaults: `replace=true`, `verify=true`, target `com.apple.TextEdit`. Focus/role settle overrides stay internal unless a test requires exposure.

## Typed AX surface (macOS)

```text
MacosDriverSession::accessibility()
  capture_app_tree(app, max_depth, max_children) -> ObservedAxTreeSnapshot
  focus_node_path(pid, path, expected_role) -> InputActionResult
  focus_text_by_query(app, query, expected_role, candidate) -> AxFocusObservation
  verify_text(app, expected_text, expected_role) -> AxTextObservation
```

`auv-driver-macos::native` remains an implementation detail.

## Evidence

The handler stages:

- `input-action-result` for focus / paste steps (`InputActionResult`)
- `ax-text-observation` for verify observation

The product finalize hook runs after those artifacts are recorded but before the
command span and run finish. It derives semantic status from the staged AX
observation, updates `InvokeResult`, and stages `operation-result` with
`VerificationResult { method: AxText, ... }` on the same run. The hook also runs
for failed handler results; if finalization itself fails, recorded invoke
persists an error run and returns the failure to the caller.

`InvokeCommandOutput.verification` remains the human boundary string; semantic success is the staged `VerificationResult`.

## Lifecycle closeout decision

- **Shared helper in core:** `InvokeFinalizeHook` stays in `auv-cli-invoke`
  because CLI and MCP must finalize before the shared span/run status decision.
- **App-specific:** AX observation decoding and TextEdit `OperationResult`
  construction stay in the TextEdit product integration.
- **Rejected:** patching `run.json` after `invoke_recorded` finishes; that leaves
  a completed run visible before canonical evidence exists.
- **Rejected:** making the handler return an unstructured error for semantic
  mismatch; that loses the failed `VerificationResult` evidence.
- **Deferred:** broader `OperationResult` schema graduation; this slice adds no
  contract fields or variants.

Validation locks handler failure, finalize failure, semantic mismatch status
parity, same-run artifact lineage, and CLI/MCP finalized artifact parity in
`auv-cli-invoke` unit tests and `textedit_document_write_parity`.

## Non-goals

README cleanup, Windows/Linux AX parity, candidate-action restore, Notes/other apps, broad OperationResult v1 graduation, planner/retry policy.
