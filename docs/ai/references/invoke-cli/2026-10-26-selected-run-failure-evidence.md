# Selected Run failures in invoke frontends

## Scope

[PR #428](https://github.com/moeru-ai/auv/pull/428) fixes error propagation
from shared preselection into the CLI and MCP invoke frontends.
This is a bug fix for an existing frontend gap.

The comparison base is `f939ae199b206fa74d63a079979c02d1c1f92c48`.
The initial PR head is `0e6702eac459898e106cca1c369602ec1a3eee7a`.

## Current behavior

`resolve_selected_context` preserves `SelectedClientError`.
`resolve_invoke_context` converts placement and context errors through the
existing `PlacementError -> InvokeFailure` policy.
Unclassified selection errors retain `command_failed`.
The inline `runner-placement-failure-codes` marker records that deferral.

| Caller | Before | After |
|---|---|---|
| `auv --run <missing-id> invoke display.list --json` | Exit 1, empty stdout | Exit 1, JSON with `status = failed` and `failure_details.code = not_found` |
| `auv --run <missing-id> invoke display.list` | Exit 1, human error on stderr | Same output and exit behavior |
| MCP `invoke` through `auv --run <missing-id> mcp serve` | `command_failed` | `isError = true`, with structured `failure_details.code = not_found` |

The external plugin boundary retains its string errors.
No public Rust signatures, CLI arguments, Protobuf definitions, or Runner
APIs change.

Preselection failure returns before local command execution or command tracing.
The failure envelope contains a generated `run_id` because the existing result
types require one. It does not prove that a daemon Run exists.

## Regression evidence

Both tests use the valid, absent Run ID
`0123456789abcdef0123456789abcdef`.
Each child uses private discovery and profile paths, with `AUV_CONTEXT` and
`AUV_ENDPOINT` removed.
The MCP child also uses an explicit temporary tracing store.

With the base production files and PR tests, both tests failed:

- CLI: JSON parsing failed because stdout was empty.
- MCP: the returned code was `command_failed`, not `not_found`.

With the PR production files, both tests passed on Windows.
The review also found a Rust formatting error that blocked all three CI hosts.

The final tests assert the command ID, legacy failure text, structured failure,
and absence of command results and artifacts.
Empty trace stores provide separate evidence that preselection did not enter
the local command path.
The MCP handshake, request, cancellation, and child exit each have a ten-second
timeout. The test also requires a successful server exit.

```text
cargo test --locked -p auv-cli --test root_cli --test integrated --no-fail-fast missing_selected_run
```

These tests do not deliver input, capture images, or start a daemon.
They establish frontend error behavior, not driver capability support.

## Selected keyboard dry-run CI regression

The [macOS CI job](https://github.com/moeru-ai/auv/actions/runs/40325285278/job/121580283670)
failed at head `7c819698cebc7e64e28a5cc6ec6f964c8d3f0967`.
The existing `selected_keyboard_dry_runs_never_fall_back_to_local_validation`
test still expected the connection error on stderr, despite its `--json` flag.

The test now reads the structured failure from stdout.
It covers `input.key`, `input.keys`, and `input.keyboard` on Unix and Windows.
Private profile and discovery paths isolate the subprocesses.
The unreachable endpoint uses a Unix socket or a unique Windows named pipe.
The legacy stderr assertion also failed on Windows after the platform guard
was expanded, before the assertions changed.

The assertions require exit 1, the selected command ID, the connection error,
and an empty command trace store.
The connection error retains its existing `command_failed` classification.
This follow-up changes test coverage only.

## Validation on Windows

Local validation used Rust 1.98.1 with the Windows helper rebuilt from this
checkout. The helper path was explicit for the package tests.

| Command | Result |
|---|---|
| `cargo fmt --check` | Passed |
| `cargo check --locked` | Passed |
| `cargo build --locked -p auv-device-helper-windows --bin auv-helper` | Passed |
| `cargo test --locked -p auv-cli -p auv-cli-invoke -p auv-core --no-fail-fast` | 421 passed, 2 ignored |
| `cargo run --quiet --locked -- invoke --help` | Passed |
| `git diff --check` | Passed |

Cross-platform workspace tests remain a CI responsibility.
