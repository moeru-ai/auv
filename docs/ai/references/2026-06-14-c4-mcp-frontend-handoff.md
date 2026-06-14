# C4 MCP Frontend Over the Same Core Command — Handoff

Date: 2026-06-14
Status: **completed locally and validated**
Roadmap anchor: `docs/ai/references/2026-06-13-auv-core-lane-roadmap.md`
Prerequisite closure: `docs/ai/references/2026-06-14-c3-steam-core-lane-closure.md`
MCP surface note: `docs/ai/references/2026-06-11-mcp-frontend-surface-v0.md`

## Locked scope for this slice

This handoff covers **C4 only**:
- prove `steam.library.list.v0` is consumable through MCP
- prove MCP calls the same core command path the CLI uses
- prove inspect output is parity-grade at the semantic shape level
- keep MCP as a thin frontend with no capability logic of its own

Explicitly **not** in scope:
- no new MCP capability invention
- no MCP-specific store/runtime path
- no mutation/replay protocol redesign
- no new Steam behavior beyond the existing C3 command path
- no C2e / Runtime collapse work

## What changed

Changed file:
- `src/mcp.rs`

Landed behavior:
- The MCP `invoke` tool was already calling `Runtime::invoke(InvokeRequest { ... })` directly.
- The C4 closure adds an explicit parity test that runs the same command through:
  1. CLI-side runtime invocation in-process
  2. MCP `invoke`
  3. MCP `run_inspect`
- The test now proves:
  - CLI and MCP both execute `steam.library.list.v0` through the same core command entry
  - MCP `output_summary` matches the CLI invoke result
  - MCP artifact role and artifact filename match the CLI invoke result
  - MCP inspect text normalizes to the same semantic run text as the CLI inspect path

## Code facts that close C4

### Same invoke path

CLI-side command execution still goes through `Runtime::invoke`:
- `src/main.rs:349-372`

MCP `invoke` also goes through `Runtime::invoke` with the same `InvokeRequest` shape:
- `src/mcp.rs:56-99`

The core runtime still resolves and dispatches the command centrally:
- `src/runtime.rs:296-355`
- `src/runtime.rs:357-435`

The command still resolves to the same honest backend from C3:
- `src/catalog.rs:910-915`
- `src/driver/steam.rs:21-90`

### Same inspect path

CLI inspect reads through:
- `src/main.rs:370-372`

MCP `run_inspect` reads through:
- `src/mcp.rs:101-112`

Both use the same underlying read-side renderer:
- `src/inspect.rs:62`

## Parity evidence added in test coverage

Primary regression test:
- `src/mcp.rs:332-500`

The test now does all of the following in one store root:
- build a CLI-side runtime with `build_runtime_with_store_root(...)`
- invoke `steam.library.list.v0` directly via `Runtime::invoke(...)`
- inspect the CLI run via `inspect_run(...)`
- start an MCP server
- invoke the same command through MCP
- inspect the MCP run through `run_inspect`
- compare the two fronts at the parity boundary

Assertions fixed by C4:
- same command id is executed: `steam.library.list.v0`
- same summary semantics: MCP `output_summary == cli_result.output_summary`
- same artifact role: `steam-library-list`
- same artifact filename shape: artifact output names match across CLI/MCP
- same inspect semantics after normalizing volatile ids/timestamps/span numbering

Important refinement:
- The test does **not** require identical run ids between CLI and MCP. They are separate executions and should produce distinct run ids.
- The real parity boundary is: same core command path, same structured result shape, and inspect text that is semantically identical after removing volatile run-local identifiers.

## Why this satisfies the C4 acceptance

Roadmap acceptance said:
- an MCP client invokes `steam.library.list.v0`
- gets the same structured result
- and a run id that inspects identically to the CLI path
- with no capability logic in the MCP layer

C4 now satisfies that as follows:
- MCP client invocation is covered by the integration test in `src/mcp.rs`
- structured result parity is covered by matching `output_summary`, artifact role, and artifact filename shape against the CLI-side invoke result
- inspect parity is covered by comparing normalized CLI/MCP inspect text from the same store root
- no capability logic was added to MCP; it still forwards explicit `command_id` into `Runtime::invoke`

## Validation run for C4

Passed:
- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`

Focused regression check passed:
- `cargo test mcp_server_lists_and_invokes_shared_runtime -- --nocapture`

## Parallel review summary

Parallel subagent review was run after the parity test landed.

Accepted conclusions:
- The new test really does add CLI-vs-MCP parity evidence because it executes both fronts in one store root and compares the semantic boundary, not only MCP-side smoke.
- No review found a new blocker in the C4 working-tree diff.
- The security review raised two existing MCP-surface observations worth tracking, but they are not new regressions introduced by this C4 slice:
  - caller-controlled `store_root`
  - the archived `candidate_action_run` tool remaining exposed on the MCP surface

Main-thread arbitration result:
- approve the C4 slice as closed
- record the MCP-surface security observations as follow-up notes only
- do not reopen C4 for broader archived-surface cleanup in this slice

## Skill / tooling route actually useful for C4

Most useful in this slice:
- `functions.Bash` — fastest truth source for validation and repo-state checks
- `mcp__auv-temp__invoke` — closest external-agent-shaped MCP path for future smoke/evidence expansion
- targeted review agents (`ecc:code-explorer`, `ecc:code-reviewer`, `ecc:rust-reviewer`) — good for path audit, test-gap review, and Rust correctness review

Not worth using as the primary path for this slice:
- broad deep-research workflows
- product-planning / marketing style skills
- any archived AX / candidate-action expansion path unrelated to `steam.library.list.v0`

## Follow-up observations (not part of C4)

- The current parity proof is semantic parity, not same-run-id parity; that is the correct boundary because CLI and MCP are separate invocations.
- The inspect normalization helper intentionally ignores volatile run-local ids, event ids, and span numbering so the test stays pinned to semantics instead of recorder-local sequencing.
- `steam.local` remains Steam-specific and `LibraryQuery::default()` remains deferred exactly as recorded in C3; C4 does not broaden either boundary.

## One-line truth of repo state now

C4 is locally closed: MCP can invoke `steam.library.list.v0` through the same core command path the CLI uses, and the resulting inspect output matches the CLI path at the semantic parity boundary without introducing MCP-local capability logic.
