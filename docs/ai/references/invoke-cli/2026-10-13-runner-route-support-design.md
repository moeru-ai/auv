# Runner route support: proposal

Status: proposal for the owner, 2026-10-13. Nothing here is implemented. It answers
`TODO(invoke-support-runner-route)` (`crates/auv-cli-invoke/src/support.rs:6`), which the
[Windows parity consolidation review](2026-10-13-windows-parity-consolidation-review.md) ranks
among its first three missing pieces. Names in this note (`runner` cells, `Direct`) are
provisional.

Evidence level: **source read** on `main` at `4c29a098`. No command was run through a Runner for
this note.

## The gap

A command's `support` cells describe its direct local path
([invoke command support](2026-10-11-invoke-command-support.md)). With a selected Run or Device,
`auv invoke` and the MCP frontend do not take that path. They send every command to
`auv_cli_invoke::runner::invoke`, which calls the selected Runner over gRPC
(`crates/auv-cli/src/commands/invoke.rs:82`, `crates/auv-cli/src/commands/mcp.rs:432`). Nothing
declares what that route supports, and the direct cells can be wrong for it in both directions.

## What the Runner route does today

- **Selection.** The adapter has one match arm per core command (`runner.rs`). The keyboard,
  drag, scroll and `scroll_until` arms are matched first and make their own Runner calls; the rest
  share one resolved Runner.
- **No platform gate.** The adapter has no `cfg` and does not read the Runner's platform. Whether a
  command runs depends only on what the Runner answers.
- **The Runner serves every service on every platform** (`local_driver.rs:2804`–`2817`). A service
  that has no path answers `Status::unimplemented`:
  - accessibility (`:304`), application (`:360`) and permission (`:574`) on Linux and Windows;
  - overlay (`:97`, `:109`) off macOS;
  - media control (`:457`, `:473`) on Linux;
  - screen-point scrolling (`:952`) on macOS.

  `driver_status` (`:2674`) also maps `DriverError::Unsupported` to `unimplemented`.
- **Every refusal arrives as `command_failed`.** Each arm turns the gRPC status into a string
  (`format!("…/… failed: {status}")`), and `InvokeFailure::from(String)` assigns `command_failed`.
  The Runner's `unimplemented` never becomes `unsupported`.
- **The Runner's platform is known before the call.** A Device carries `DevicePlatform`
  (`proto/auv/api/daemon/v1/device.proto:45`), so a frontend could refuse before dispatch.

## Where the Runner route and the direct cells differ

Compared for this note: the commands below. The remaining core commands call the same driver
capability through the Runner as they do directly, judging by the RPC each arm calls. That was not
checked line by line; the first implementation step does it.

| Command | Direct cell | Runner route (source) |
| --- | --- | --- |
| `screen.clickText` | Linux, Windows: unsupported | No gate (`runner.rs:301`): `FindDisplayText`, then `ClickPoint` in screen space, `ForegroundPreferred` by default (`runner.rs:616`). Linux and Windows Runners serve both, so a path exists. |
| `window.clickText` | Linux, Windows: unsupported | No gate (`runner.rs:358`): `ResolveWindow`, `FindWindowText`, then a window-relative `ClickPoint`. Linux and Windows Runners serve all three. The default policy is `BackgroundPreferred` (`runner.rs:617`), so on Windows the click is posted unless it falls back to the foreground. |
| `window.findText`, `window.waitForText` | Linux: unsupported (handler compiled for macOS and Windows only, `commands/window.rs:231`, `:272`) | `FindWindowText` (`runner.rs:235`). A Linux Runner captures the window and runs the Linux driver's OCR on it (`local_driver.rs:2129`, `auv-driver-linux/src/vision.rs:34`), so a path exists. |
| `app.activate`, `app.probePermissions`, `input.focusText` | Linux, Windows: unsupported (refused as `unsupported` by #355) | Same state: the Runner answers `unimplemented`. It arrives as `command_failed`. |
| `mediaControl.*` | Linux: unsupported | Same state, arriving as `command_failed`. |
| `scan.frame`, `scan.coverage` (fixtures) | supported everywhere | No arm, so with a live Runner they would reach `unreachable!` and panic the frontend; #357 refuses them as `unsupported` before daemon resolution. |

Two consequences:

1. **A Runner may already run the two clickText commands on Linux and Windows,** and
   `window.findText` on Linux, although the direct cells say unsupported. Cells are statements
   about source, so the honest Runner cells for these are *not* `unsupported`. Whether they work
   is not evidenced: no run went through a Runner.
2. **The Runner adapter already holds the sequence that the review's wiring changes W1 and S1
   would add to the direct `window.clickText` and `screen.clickText`** (resolve, recognize, click). Wiring the direct handlers should reuse it, not write a second one.

## Proposal

### 1. Typed refusals on the Runner route (first, no new surface)

Map the gRPC status code once, in the adapter, instead of formatting it into a string:

| gRPC code | `FailureCode` |
| --- | --- |
| `UNIMPLEMENTED` | `unsupported` |
| `NOT_FOUND` | `not_found` |
| `PERMISSION_DENIED` | `permission_denied` |
| `INVALID_ARGUMENT` | `invalid_input` |
| anything else | `command_failed`, as today |

The message keeps the RPC name. This is the Runner-route half of #355, and is bug-fix sized.

### 2. Runner cells

Add a second declaration to `invoke_command`, keyed by the Runner's platform:

```rust
support = { macos: Supported, linux: Unsupported, windows: Unsupported("…") },
runner  = { macos: Supported, linux: Partial("…"), windows: Partial("…") },
```

- **Required on every core command,** like `support`, so a new command cannot leave its Runner
  route undeclared. A shorthand `runner = Direct` says "the Runner calls the same driver capability,
  so the cells are the direct ones". It may be used only where the audit confirmed it.
- **Fixture commands** declare no Runner route (`runner = None`), which the frontends refuse as
  `unsupported`, as #357 does.
- **Help** shows a "Runner support" block only where it differs from the direct cells. **MCP
  metadata** always carries both (`support`, `runner_support`).
- **Registry tests** extend the existing ones: every core command declares a Runner route, and
  every command with an adapter arm declares one that is not `None`.

### 3. Refusal before dispatch

With a selected Device, the frontend knows `DevicePlatform`. A command whose Runner cell for that
platform is `unsupported` is refused as `unsupported` before the call, like the existing "before
daemon resolution" checks in `runner_test.rs`. Step 1 remains the backstop when a Runner and a
client disagree.

### 4. First cells

Following the source above, before any run:

- `window.clickText` Runner cells on Linux and Windows: `Partial`, naming the delivery limit (on
  Windows, the existing "background delivery posts window messages…" note).
- `screen.clickText` Runner cells: `Partial("primary display only")` on Windows; Linux to be
  read in the audit.
- `window.findText` and `window.waitForText` Runner cell on Linux: `Supported` if the audit confirms
  the AT-SPI capture path; `docs/SUPPORT_MATRIX.md` keeps "not claimed" until a run.

## Open questions for the owner

1. **Required or defaulted.** This note recommends required cells with a `Direct` shorthand. The
   alternative, "same as direct unless overridden", is less work but is how the clickText gap went
   unnoticed.
2. **Fixture commands with a selection.** Refuse them (current after #357), or run them locally
   inside the selected Run, since they touch no desktop?
3. **The direct Linux gate on `window.findText`.** The Linux driver has the pieces. Was the gate
   deliberate, or is it the same stale gate #336 removed on Windows? It needs a Linux run either way.
4. **Names.** `runner` cells, `runner_support` in MCP, `Direct`, `None`.

## Slices, once approved

1. Typed status mapping in the adapter, with a test per mapped code.
2. The `runner` declaration, help and MCP rendering, and registry tests, with every cell read from
   source.
3. Refusal before dispatch from `DevicePlatform`.
4. Runs through a Runner for the cells that become `Partial` or `Supported`, recorded in
   `docs/SUPPORT_MATRIX.md` with their evidence level.
