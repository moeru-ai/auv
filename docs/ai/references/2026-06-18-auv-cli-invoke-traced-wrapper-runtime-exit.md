# auv-cli-invoke Traced Wrapper and Runtime Exit Design

Date: 2026-06-18
Status: accepted design boundary; implementation plan pending

## Decision

`auv-cli-invoke` should not own invoke command business semantics.

Each `<domain>.<action>` command keeps its own Rust handler as the semantic
owner. The handler may call `auv-driver`, `auv-driver-macos`,
`auv-driver-windows`, `auv-inference-*`, or other typed APIs, and it is
responsible for producing the command-level `InvokeCommandOutput`: summary,
signals, notes, artifacts, known limits, and boundary claims.

`auv-cli-invoke` may own a common traced invocation wrapper that all frontends
can reuse. That wrapper resolves or accepts an `InvokeCommand`, opens the
standard command span, calls the handler, records the handler output through
`auv-tracing-driver`, and maps the result into the shared `InvokeResult`.

This wrapper is deliberately not a planner, runtime, recipe executor, bundle
lookup, or legacy command engine.

## Current Problem

`src/runtime.rs` still hosts the invoke recording wrapper:

- standalone invoke run creation and finish
- registry lookup for `InvokeRequest.command_id`
- `auv.command.invoke` span creation and finish
- command output event recording
- `ProducedArtifact` staging through `RunRecordingBackend`
- `InvokeCommandOutput` to `InvokeResult` mapping

This makes `Runtime` appear necessary for invoke even though direct command
handlers already live in `auv-cli-invoke`, and durable recording primitives
already live in `auv-tracing-driver`.

## Target Boundary

The target split is:

| Layer | Responsibility |
| --- | --- |
| `InvokeCommand` handler | Execute one `<domain>.<action>` command through typed Rust APIs and return `InvokeCommandOutput`. |
| `auv-cli-invoke` traced wrapper | Resolve command, open/finish invoke span, call handler, record events/artifacts/boundary claims, return `InvokeResult`. |
| `auv-tracing-driver` | Provide run/span/event/artifact recording primitives and store persistence. |
| CLI / MCP / app / scroll scan | Choose standalone run versus existing run/span and call the wrapper. |

`auv-cli-invoke` can therefore provide wrapper APIs shaped like:

```rust
pub fn invoke_recorded(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  request: InvokeRequest,
) -> AuvResult<InvokeResult>;

pub fn invoke_recorded_in_span(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  run: &mut RecordingRun,
  parent: &SpanRef,
  request: InvokeRequest,
) -> AuvResult<InvokeResult>;

pub fn invoke_resolved_recorded_in_span(
  recording: &RunRecordingBackend,
  run: &mut RecordingRun,
  parent: &SpanRef,
  command: &InvokeCommand,
  request: InvokeRequest,
) -> AuvResult<InvokeResult>;
```

Names are provisional. The important boundary is that these APIs are wrappers
around handler execution and tracing, not owners of domain semantics.

## Migration Order

1. Add the traced wrapper to `auv-cli-invoke`.
2. Move the existing `src/runtime.rs` invoke wrapper tests to the new wrapper
   where possible.
3. Change `src/main.rs` `invoke` to construct `RunRecordingBackend` directly
   and call `auv-cli-invoke`.
4. Change `src/mcp.rs` generic invoke tool to call `auv-cli-invoke` directly.
5. Change `src/main.rs` resolved invoke call sites, such as the Minecraft
   `input.clickWindowPoint` path, to call the resolved wrapper.
6. Change `src/app/infra.rs` and `src/scroll_scan/mod.rs` later to call the
   existing-span wrapper, or to manage their own spans with `auv-tracing-driver`
   and then call command handlers through the wrapper.
7. Delete the migrated invoke methods and helper code from `src/runtime.rs`.

## Runtime Cleanup Enabled By This Boundary

After invoke callers migrate, these `Runtime` methods should disappear instead
of becoming compatibility shims:

- `invoke`
- `invoke_resolved`
- `invoke_in_span`
- private `invoke_in_command_run`
- private `invoke_metadata_command_in_span`
- invoke-only helper functions such as command attribute and event recording
  helpers, unless they move into `auv-cli-invoke`

The same runtime exit pass should separately migrate read-side and business
facade methods that already have direct replacements:

- `inspect` -> `crate::inspect::inspect_run`
- `read_run` -> `RunRecordingBackend::read_run` or `LocalStore::read_run`
- `list_*` read helpers -> `crate::inspect::*` or `crate::run_read::*`
- `run_recorded_operation` -> `RunRecordingBackend::handle().run_recorded_operation`
- candidate action artifact facades -> candidate action modules plus
  `auv-tracing-driver`
- Minecraft telemetry/projection artifact facades -> owning Minecraft or CLI
  helper modules plus `auv-tracing-driver`

## Non-Goals

- Do not reintroduce legacy command compatibility.
- Do not move domain-specific command logic into the wrapper.
- Do not make `auv-cli-invoke` a planner or recipe engine.
- Do not make `auv-tracing-driver` know about invoke command ids or registry
  lookup.
- Do not add bundle, recipe, skill, or catalog compatibility.

## Open Implementation Detail

`InvokeRequest` and `InvokeResult` currently live in the root crate. The
implementation plan must choose one of these routes:

1. Move the shared invoke request/result models to `auv-cli-invoke` or a small
   shared contract crate.
2. Keep the first wrapper implementation in the root crate as a temporary
   extraction step, then move it once the type boundary is clean.
3. Introduce wrapper-specific input/output types in `auv-cli-invoke` and adapt
   root CLI/MCP at the boundary.

The preferred long-term shape is for `auv-cli-invoke` to expose the wrapper
without depending on the root `auv-cli` crate.
