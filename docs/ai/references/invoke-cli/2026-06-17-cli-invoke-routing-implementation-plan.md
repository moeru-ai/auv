# auv-cli-invoke Metadata Routing Implementation Handoff

Date: 2026-06-17

Status: completed in local worktree branch
`refactor/auv-cli-invoke-metadata-routing`.

## Goal

Refactor `auv-cli-invoke` so `#[invoke_command]` registers invoke-visible
command metadata and argument schema without generating driver dispatch
handlers or carrying execution contract fields.

## Completed Shape

- `auv-cli-invoke` now owns the invoke sub-command surface: command ids,
  groups, summaries, argument schema, help rendering, registry resolution, and
  invoke-specific argv parsing.
- `#[invoke_command]` accepts only `id`, `group`, `summary`, and `args`.
- Execution metadata fields were removed from the macro and command metadata:
  `driver`, `operation`, `disturbance`, `max_disturbance`, `artifacts`,
  `signals`, `verification`, and `operation_namespace`.
- Annotated functions started as private metadata anchors in this slice. The
  follow-up handler-binding slice replaces those inert anchors with real invoke
  implementation functions registered by `*_invoke_command()` constructors.
- Root runtime no longer depends on `InvokeDriverDispatch`. This slice kept a
  temporary command-id-to-driver-operation table in `src/runtime.rs`; the
  handler-binding follow-up moves that driver operation request into each
  command function.
- Root CLI delegates invoke-specific argument parsing to
  `auv_cli_invoke::parse_invoke_args` while keeping inspect/storage options in
  root CLI parsing.
- MCP command metadata serialization was updated to use the reduced invoke
  metadata while preserving runtime evidence tests.

## Important Boundaries

- `src/driver/macos` was not deleted in this slice. That migration needs typed
  driver/domain API coverage before the runtime driver-operation adapter can be
  removed.
- `auv-cli-invoke` is still the CLI-facing invoke surface, but it is not the
  execution router.
- The temporary root runtime route table is intentionally outside
  `auv-cli-invoke` so command metadata cannot drift back into driver dispatch.
- Composite skills such as future scroll scan should be implemented as typed
  domain or interaction capabilities first, then exposed through invoke
  metadata, instead of making the proc macro schedule execution.

## Follow-Up: Handler Binding

The metadata-only refactor intentionally removed macro-owned driver dispatch.
The next slice binds each metadata entry to its implementation function so empty
macro anchors disappear while driver routing remains outside macro attributes.
During the transition, command functions may return a temporary
`DriverOperation` request; that value is executable code, not
`#[invoke_command]` metadata.

## Verification

The implementation was validated with:

- `cargo fmt --check`
- `cargo test -p auv-cli-invoke -p auv-cli-invoke-macros`
- `cargo check`
- `cargo test parse_invoke -- --nocapture`
- `cargo test mcp -- --nocapture`
- `cargo test`
- `git diff --check`
- `cargo run --quiet -- invoke --help`

`cargo run --quiet -- list-commands` still exits with the existing tombstone
error because `list-commands` has been removed by prior project design:

```text
`list-commands` has been removed; use `auv-cli invoke --help` instead.
```

## Follow-Ups

- Replace driver-operation handler bodies with typed runtime/domain capability
  calls as those APIs become available.
- Add typed APIs to driver/domain crates for invoke commands that still depend
  on the root macOS driver path.
- Delete or quarantine root `src/driver/macos` only after the active invoke
  commands no longer depend on its driver operations.
