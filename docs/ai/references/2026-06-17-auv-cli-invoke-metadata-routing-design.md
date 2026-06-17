# auv-cli-invoke Metadata Routing Design

Date: 2026-06-17

## Status

Implemented direction for the `auv-cli-invoke` metadata-routing refactor slice.
This note records the accepted boundary; see
`2026-06-17-auv-cli-invoke-routing-implementation-plan.md` for the completed
handoff and verification record.

## Problem

Before this refactor, `auv-cli-invoke` modeled invoke commands as metadata plus
a handler that returned `InvokeDriverDispatch`. Most command functions only
called `default_driver_dispatch`, which made the function look executable while
it was really a metadata-to-driver-call adapter.

That shape has two problems:

- It treats every invoke command as a single driver operation.
- It makes `#[invoke_command]` responsible for scheduling-facing dispatch
  details instead of invoke command registration.

The result is a confusing boundary: `auv-cli-invoke` looks like both a command
catalog and an execution adapter.

## Desired Boundary

`auv-cli-invoke` owns the invoke sub-command surface:

- invoke-visible command registration
- command id and group routing
- invoke-specific argument schema
- invoke help rendering
- a narrow adapter that `src/main.rs` can use for the `invoke` sub-command

`auv-cli-invoke` does not own:

- global CLI framework behavior
- platform execution policy
- generic driver dispatch
- artifact, signal, verification, or disturbance contracts

Those runtime outputs belong to the command implementation and to the
capabilities it calls.

This boundary can improve before all root driver adapters are replaced with
typed domain capabilities.
If a command still executes through root runtime compatibility code, that
compatibility belongs outside `auv-cli-invoke`; it must not leak back into
`#[invoke_command]` metadata.

## Macro Contract

`#[invoke_command]` marks an invoke implementation function as routable from the
invoke sub-command registry. The macro standardizes metadata and registers the
function pointer; the function body owns execution.

```rust
#[invoke_command(
  id = "steam.library.list.v0",
  group = "steam",
  summary = "List installed Steam library apps from local appmanifest files.",
  args = NO_ARGS,
)]
fn library_list(
  input: InvokeCommandInput<'_>,
) -> Result<InvokeCommandExecution, String> {
  // Command implementation calls the relevant domain crate or capability.
}
```

The generated `library_list_invoke_command()` constructor is public registry
metadata plus the implementation function pointer.

The macro should not require or generate:

- `driver`
- `operation`
- `disturbance`
- `max_disturbance`
- `artifacts`
- `signals`
- `verification`
- `InvokeDriverDispatch`
- `default_driver_dispatch`

## Execution Model

The long-term invoke command implementation owns how the command is fulfilled.
It may call `auv-driver`, `auv-driver-macos`, future platform drivers,
`auv-media-macos`, or future interaction crates such as `auv-interactions`.

This keeps `auv-cli-invoke` as an external interface wrapper over project
capabilities instead of a thin hardcoded proxy to `auv-driver`.

Runtime remains responsible for run recording, span ownership, artifact
persistence, inspection-compatible invoke results, and adapting temporary driver
operation requests while commands migrate to typed domain execution.

The temporary `DriverOperation` return value is an implementation detail
inside command functions. It is not macro metadata and should be replaced
command-by-command as typed domain capabilities become available.

## Superseded Detail

The temporary `DriverOperation` adapter has now been removed from
`auv-cli-invoke` and root runtime invoke handling. `#[invoke_command]` still
registers metadata plus a function pointer, but the function returns
`InvokeCommandOutput` directly. Commands with no typed capability yet return an
explicit typed-API gap error instead of routing through root driver operation
strings.

## CLI Adapter

`src/main.rs` should stay thin for the `invoke` sub-command. The intended shape
is that `auv-cli-invoke` exposes invoke-specific parsing/help/routing helpers
that `src/main.rs` can call directly.

`auv-cli-invoke` may parse invoke command arguments from an argv slice and render
invoke help, but it should not own the whole process CLI or bind the repository
to a new CLI framework.

## First Refactor Slice

The first implementation slice should remove the misleading dispatch model from
`auv-cli-invoke` without trying to delete every root driver adapter at the same
time:

- Delete `InvokeDriverDispatch`.
- Delete `InvokeCommandHandler`.
- Delete `InvokeCommand::dispatch`.
- Delete `InvokeCommand::with_handler`.
- Delete `default_driver_dispatch`.
- Narrow `#[invoke_command]` to id, group, summary, and args.
- Keep any remaining root-runtime execution compatibility outside
  `auv-cli-invoke` until each command has a typed capability implementation.

Do not introduce a separate "plan" or composite execution abstraction in this
slice. Future multi-capability commands should be implemented by their command
function or by a domain crate that the command function calls.

## Directory Shape

The model cleanup should happen before a broad file move.

After the macro and execution boundary are clean, command files can move toward
domain-owned directories:

```text
crates/auv-cli-invoke/src/commands/
  input/
    mod.rs
    operations.rs
    type_text.rs
    press_key.rs
  window/
    mod.rs
    operations.rs
    find_text.rs
```

That directory split is a follow-up slice, not a prerequisite for deleting the
dispatch abstraction.
