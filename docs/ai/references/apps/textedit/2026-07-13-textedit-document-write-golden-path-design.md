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
| Product registry extension | `auv-product` (`product_registry` = core + TextEdit) |
| Recording | `auv-cli-invoke::invoke_recorded` |
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

Handler stages:

- `input-action-result` for focus / paste steps (`InputActionResult`)
- `ax-text-observation` for verify observation
- `operation-result` with `VerificationResult { method: AxText, ... }`

`InvokeCommandOutput.verification` remains the human boundary string; semantic success is the staged `VerificationResult`.

## Non-goals

README cleanup, Windows/Linux AX parity, candidate-action restore, Notes/other apps, broad OperationResult v1 graduation, planner/retry policy.
