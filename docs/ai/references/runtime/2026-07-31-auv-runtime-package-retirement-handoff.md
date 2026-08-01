# `auv-runtime` Package Retirement Handoff

Date: 2026-07-31

Status: implemented owner-approved narrow refactor

## Decision

AUV no longer has a root Cargo package or a crate named `auv-runtime`. Runtime
remains an architectural responsibility distributed across typed operation
modules, frontend roots, drivers, and tracing. A catch-all package is not
required to connect those modules.

The evidence for retirement was the current workspace dependency graph:

1. `auv-cli` was the only default consumer of the root package.
2. MCP and session code were executable frontend implementations.
3. `src/scroll_scan` had no workspace production caller.
4. The root recognition/candidate contracts had no production consumer beyond
   their own dormant artifact producer.
5. The remaining input-action artifact path already had an active producer in
   `auv-cli-invoke`, while game crates already owned their artifact producers.

## Current ownership

| Former root surface | Current owner |
|---|---|
| `src/mcp.rs` | `crates/auv-cli/src/mcp.rs` |
| `src/api/session_service` | `crates/auv-api-server` |
| `src/model.rs` invoke re-exports | direct imports from `auv-cli-invoke` |
| `src/model.rs` result/time aliases | crate-local aliases or functions |
| input-action artifact validation | `InputActionResult::validate` in `auv-driver-common` |
| input-action artifact emission for invoke commands | `auv-cli-invoke::emit_input_action_result` |
| app/game artifact publication | the owning app/game tracing module over `auv-tracing` |
| artifact storage and dispatch | `auv-tracing` |

`auv-tracing` does not depend on driver, scan, or recognition types. It remains
the generic write-side tracing module. Typed producers validate their domain
values before calling its artifact interface.

## Retired surfaces

- Root `src/contract.rs`, including the unused shared `RecognitionResult` and
  `CandidateRef` family.
- Root `src/scroll_scan`, including its uncalled orchestration and artifact
  shape.
- Root `src/run_read`; despite its name it was a producer module, not a run
  reader. Its scan and recognition producers retired with their types, and its
  live input-action responsibility moved to existing owners.
- Root `src/lib.rs`, `src/model.rs`, the root manifest package sections, and the
  root-package dependency guard test.

These removals do not approve recreating the retired candidate-promotion seam,
generic scroll-scan orchestration, or a differently named aggregate runtime
crate. A future shared type needs a named producer, consumer, and owner.

## Execution and recording after retirement

```text
CLI / MCP / API server frontend
  -> auv-cli-invoke command or app-owned typed operation
    -> auv-driver capability
      -> direct typed result
      -> typed event/artifact emission
        -> auv-tracing Dispatch / TracingStore
```

Frontends create root tracing contexts and flush recording after direct command
completion. Recording failure remains separate from the direct result and must
not cause command re-execution.

## Supersession rule

Older specs and evidence packs remain historical records. References in those
documents to root `src/*` paths, root-owned result contracts, or an
`auv-runtime` package describe their date-specific state and are superseded by
this handoff for current implementation work.
