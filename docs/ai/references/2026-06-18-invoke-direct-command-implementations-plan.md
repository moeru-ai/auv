# Invoke Direct Command Implementations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate every retained invoke command function from string
driver-operation routing to direct Rust implementations.

**Architecture:** `#[invoke_command]` remains metadata-only plus function
registration. Each annotated function owns its command implementation and
returns `InvokeCommandOutput`. `auv-cli-invoke` may depend on domain/driver
crates, but it must not route through `driver_id` / `operation` strings.
Artifact persistence remains runtime / `auv-tracing-driver` work, not an
`auv-cli-invoke` helper. Steam invoke compatibility is not preserved; the Steam
invoke command is removed instead of migrated.

**Tech Stack:** Rust 2024, `auv-cli-invoke`, `auv-driver`,
`auv-driver-macos`, `auv-tracing-driver`, root runtime recording, existing
two-space `rustfmt` config.

---

## Required Outcome

After this slice:

- No retained command under `crates/auv-cli-invoke/src/commands/` calls
  `InvokeCommandExecution::driver_operation(...)`.
- `InvokeCommandExecution::DriverOperation` is deleted.
- Root runtime no longer contains `DriverOperationRoute` or
  `invoke_driver_operation_in_span`.
- Every retained invoke command function returns `InvokeCommandOutput`.
- Any macOS capability needed by an invoke command exists in
  `auv-driver-macos` or another owning crate before the command calls it.
- `steam.library.list.v0` is removed from `auv-cli-invoke`, its registry group,
  runtime/MCP expectations, and command tests. No compatibility route is kept.
- `auv-cli-invoke` does not define artifact writing helpers. If a migrated
  command needs durable artifacts, that is a follow-up in
  `auv-tracing-driver` / runtime.

## Non-Goals

- Do not reintroduce `driver` or `operation` macro attributes.
- Do not make the proc macro schedule execution.
- Do not build an artifact helper inside `auv-cli-invoke`.
- Do not preserve the Steam invoke command.
- Do not implement scroll scan integration.
- Do not delete all of root `src/driver/macos` merely because invoke no longer
  routes through it; delete only code that becomes unused and is clearly owned
  by this migration.

## File Structure

- Modify `crates/auv-cli-invoke/Cargo.toml`: add direct dependencies on
  `auv-driver` and platform/domain crates needed by retained commands.
- Modify `crates/auv-cli-invoke/src/command.rs`: replace
  `InvokeCommandExecution::{DriverOperation, ...}` with direct output only.
- Modify `crates/auv-cli-invoke/src/commands/*.rs`: implement each retained
  command directly.
- Delete `crates/auv-cli-invoke/src/commands/steam.rs` and remove it from the
  module tree and registry.
- Modify `crates/auv-cli-invoke/src/registry.rs`,
  `crates/auv-cli-invoke/src/command.rs`, and
  `crates/auv-cli-invoke-macros/src/lib.rs` to remove Steam group support if no
  retained command uses it.
- Modify `crates/auv-driver-macos/src/*`: expose missing typed APIs needed by
  invoke commands before calling them.
- Modify `src/runtime.rs`: record `InvokeCommandOutput` directly.
- Modify `src/mcp.rs` and tests: remove Steam command expectations and preserve
  invoke metadata/output behavior.
- Update docs in `docs/ai/references/`.

## Command Migration Inventory

Retained command domains:

- `app`: `probePermissions`, `activate`
- `display`: `capture`, `identifyPoint`, `list`,
  `probeCoordinateReadiness`, `projectScreenshotPoint`
- `screen`: `captureRegion`, `findText`, `waitForText`, `findRows`,
  `waitForRows`, `findImageText`, `clickText`, `clickRow`
- `window`: `list`, `capture`, `captureAxTree`, `findText`, `waitForText`,
  `findRows`, `waitForRows`, `observeRegion`, `findIconMatch`,
  `scrollRegion`, `verifyText`, `clickText`, `clickRow`
- `input`: `focusText`, `pressButton`, `axPressButton`, `axFocusText`,
  `axClickWindowText`, `smartPress`, `typeText`, `pasteText`, `key`,
  `clickPoint`, `clickWindowPoint`, `teachClick`, `scrollPoint`
- `overlay`: `clickPoint`, `showCursor`, `showDualCursor`,
  `applyCursorBatch`, `setCursor`, `moveCursor`, `moveCursorById`,
  `flashCursor`, `flashCursorById`, `hideCursorId`, `hideCursor`,
  `shutdown`
- `mediaControl`: `nowPlaying`, `play`, `pause`, `togglePlayPause`,
  `next`, `previous`
- `fixture`: `observe`

Removed command domains:

- `steam`: remove `steam.library.list.v0`; no compatibility path.

## Task 1: Replace Execution Contract With Output Only

**Files:**
- Modify: `crates/auv-cli-invoke/src/command.rs`
- Modify: `crates/auv-cli-invoke/src/lib.rs`
- Modify: `crates/auv-cli-invoke/tests/invoke_command_macro.rs`

- [ ] **Step 1: Replace execution enum**

In `crates/auv-cli-invoke/src/command.rs`, replace
`InvokeCommandExecution` with:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeCommandOutput {
  pub summary: String,
  pub backend: Option<String>,
  pub signals: BTreeMap<String, String>,
  pub notes: Vec<String>,
}

impl InvokeCommandOutput {
  pub fn new(summary: impl Into<String>) -> Self {
    Self {
      summary: summary.into(),
      backend: None,
      signals: BTreeMap::new(),
      notes: Vec::new(),
    }
  }
}

pub type InvokeCommandResult = Result<InvokeCommandOutput, String>;
```

Change the private handler alias to:

```rust
type InvokeCommandHandler = fn(InvokeCommandInput<'_>) -> InvokeCommandResult;
```

Change `InvokeCommand::invoke` to return `InvokeCommandResult`.

- [ ] **Step 2: Export output/result types**

In `crates/auv-cli-invoke/src/lib.rs`, export:

```rust
pub use command::{
  CommandGroup, CommandNode, InvokeCommand, InvokeCommandInput, InvokeCommandOutput,
  InvokeCommandResult, InvokeNamespace,
};
```

- [ ] **Step 3: Update macro test**

In `crates/auv-cli-invoke/tests/invoke_command_macro.rs`, make the test handler
return direct output:

```rust
fn external_generated_command_handler(
  _input: auv_cli_invoke::InvokeCommandInput<'_>,
) -> auv_cli_invoke::InvokeCommandResult {
  Ok(auv_cli_invoke::InvokeCommandOutput::new(
    "external handler ran",
  ))
}
```

Assert:

```rust
let output = command
  .invoke(auv_cli_invoke::InvokeCommandInput {
    command_id: command.id,
    target_application_id: None,
    inputs: &std::collections::BTreeMap::new(),
    dry_run: false,
  })
  .expect("handler should run");

assert_eq!(output.summary, "external handler ran");
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p auv-cli-invoke --test invoke_command_macro -- --nocapture
```

Expected: compile failures remain in command modules until later tasks migrate
their function bodies.

## Task 2: Remove Steam Invoke Command

**Files:**
- Delete or empty: `crates/auv-cli-invoke/src/commands/steam.rs`
- Modify: `crates/auv-cli-invoke/src/commands/mod.rs`
- Modify: `crates/auv-cli-invoke/src/registry.rs`
- Modify: `crates/auv-cli-invoke/src/command.rs`
- Modify: `crates/auv-cli-invoke-macros/src/lib.rs`
- Modify: `crates/auv-cli-invoke/src/lib.rs`
- Modify: `src/mcp.rs`
- Test: invoke/MCP tests

- [ ] **Step 1: Remove Steam namespace and group**

Remove `InvokeNamespace::Steam`, its `as_str` arm, the `steam` macro group
mapping, and `commands::steam::group()` from `default_registry()`.

- [ ] **Step 2: Remove command module**

Remove `pub mod steam;` from `crates/auv-cli-invoke/src/commands/mod.rs`.
Delete `crates/auv-cli-invoke/src/commands/steam.rs`.

- [ ] **Step 3: Update tests and MCP expectations**

Remove assertions that expect `steam.library.list.v0` to appear in invoke help,
registry metadata, or MCP command metadata. Remove MCP tests that invoke
`steam.library.list.v0`.

- [ ] **Step 4: Run tests**

Run:

```bash
cargo test -p auv-cli-invoke -p auv-cli-invoke-macros
cargo test mcp -- --nocapture
```

Expected: pass with Steam invoke command absent.

## Task 3: Add Runtime Direct Output Recording

**Files:**
- Modify: `src/runtime.rs`
- Test: `src/runtime.rs`

- [ ] **Step 1: Update runtime invocation path**

In `invoke_metadata_command_in_span`, remove matching on
`InvokeCommandExecution`. It should call the command handler and pass the
returned `InvokeCommandOutput` to a direct recording helper.

- [ ] **Step 2: Remove driver-operation runtime adapter**

Delete:

- `DriverOperationRoute`
- `invoke_driver_operation_in_span`
- driver span creation that exists only for invoke command routing

Keep root driver support only where other runtime paths still need it.

- [ ] **Step 3: Add direct output recording helper**

Add a helper that records only an `auv.command.invoke` span, records
`command.backend` and `command.note` events from output, and returns
`InvokeResult` with `producer_span_id` equal to the command span id.

Do not add artifact recording in this slice. `InvokeCommandOutput` has no
artifact field.

- [ ] **Step 4: Add runtime test**

Add `invoke_resolved_records_direct_command_output_without_driver_span`:

- use a test handler that returns `InvokeCommandOutput::new("direct output completed")`
- assert `result.output_summary == "direct output completed"`
- assert there is an `auv.command.invoke` span
- assert there is no `auv.driver.invoke` span

- [ ] **Step 5: Run runtime tests**

Run:

```bash
cargo test invoke_resolved -- --nocapture
```

Expected: pass after command modules compile.

## Task 4: Migrate Fixture And App Commands

**Files:**
- Modify: `crates/auv-cli-invoke/src/commands/fixture.rs`
- Modify: `crates/auv-cli-invoke/src/commands/app.rs`
- Modify: `crates/auv-cli-invoke/Cargo.toml`
- Modify: `crates/auv-driver-macos/src/*` only if app APIs are missing

- [ ] **Step 1: Add dependencies**

`crates/auv-cli-invoke/Cargo.toml` needs:

```toml
auv-driver = { path = "../auv-driver" }

[target.'cfg(target_os = "macos")'.dependencies]
auv-driver-macos = { path = "../auv-driver-macos" }
```

- [ ] **Step 2: Migrate `fixture.observe`**

Return a direct deterministic output:

```rust
Ok(InvokeCommandOutput::new(
  "fixture observed",
))
```

- [ ] **Step 3: Migrate `app.probePermissions`**

On macOS, call:

```rust
use auv_driver::Driver;

let driver = auv_driver_macos::MacosDriver::new();
let session = driver.open_local().map_err(|error| error.to_string())?;
let permissions = session.permission().probe().map_err(|error| error.to_string())?;
```

Return summary and permission signals in `InvokeCommandOutput`.

On non-macOS, return:

```rust
Err("app.probePermissions is only available on macOS".to_string())
```

- [ ] **Step 4: Migrate `app.activate`**

If `auv-driver-macos` already exposes app/window activation through typed APIs,
call it. If not, first move the activation capability from root
`src/driver/macos` into `auv-driver-macos`, then call it from
`crates/auv-cli-invoke/src/commands/app.rs`.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test -p auv-cli-invoke fixture -- --nocapture
cargo test -p auv-cli-invoke probe_permissions -- --nocapture
```

Expected: pass.

## Task 5: Migrate Display And Media Control Commands

**Files:**
- Modify: `crates/auv-cli-invoke/src/commands/display.rs`
- Modify: `crates/auv-cli-invoke/src/commands/media_control.rs`
- Modify: `crates/auv-driver-macos/src/*`
- Test: domain-specific invoke tests

- [ ] **Step 1: Inventory and move missing typed APIs**

For each display/media command, map the old root driver operation to an
existing or new typed crate API:

```text
display.list -> auv-driver-macos DisplayApi::list
display.capture -> DisplayApi::capture
display.identifyPoint -> add typed display point identification API if missing
display.probeCoordinateReadiness -> add typed coordinate readiness API if missing
display.projectScreenshotPoint -> add typed screenshot projection API if missing
mediaControl.* -> add typed media control API in auv-driver-macos or a media crate if missing
```

Do not call root `src/driver/macos` from `auv-cli-invoke`. If a capability only
exists in root, move it into `auv-driver-macos` or the owning domain crate
first.

- [ ] **Step 2: Migrate every display command handler**

Each checked handler must parse its own `InvokeCommandInput.inputs`, call a
typed API, and return `InvokeCommandOutput`.

- [ ] `display.capture`
- [ ] `display.identifyPoint`
- [ ] `display.list`
- [ ] `display.probeCoordinateReadiness`
- [ ] `display.projectScreenshotPoint`

- [ ] **Step 3: Migrate every media control command handler**

Each checked handler must call a typed media control API and return
`InvokeCommandOutput`.

- [ ] `mediaControl.nowPlaying`
- [ ] `mediaControl.play`
- [ ] `mediaControl.pause`
- [ ] `mediaControl.togglePlayPause`
- [ ] `mediaControl.next`
- [ ] `mediaControl.previous`

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p auv-cli-invoke display -- --nocapture
cargo test -p auv-cli-invoke media -- --nocapture
cargo test display -- --nocapture
cargo test media_control -- --nocapture
```

Expected: pass.

## Task 6: Migrate Screen And Window Commands

**Files:**
- Modify: `crates/auv-cli-invoke/src/commands/screen.rs`
- Modify: `crates/auv-cli-invoke/src/commands/window.rs`
- Modify: `crates/auv-driver-macos/src/*`
- Test: OCR/window/runtime tests

- [ ] **Step 1: Move observation and OCR command helpers into owning crate APIs**

For each `screen.*` and `window.*` command, ensure the required OCR, row,
capture, AX tree, icon match, observe region, scroll region, and verification
logic lives in `auv-driver-macos` or a domain crate, not root
`src/driver/macos`.

- [ ] **Step 2: Migrate every screen command handler**

Each command handler should parse `InvokeCommandInput.inputs`, call the typed
API, and build `InvokeCommandOutput`.

- [ ] `screen.captureRegion`
- [ ] `screen.findText`
- [ ] `screen.waitForText`
- [ ] `screen.findRows`
- [ ] `screen.waitForRows`
- [ ] `screen.findImageText`
- [ ] `screen.clickText`
- [ ] `screen.clickRow`

- [ ] **Step 3: Migrate every window command handler**

Each command handler should parse `InvokeCommandInput.inputs`, call the typed
API, and build `InvokeCommandOutput`.

- [ ] `window.list`
- [ ] `window.capture`
- [ ] `window.captureAxTree`
- [ ] `window.findText`
- [ ] `window.waitForText`
- [ ] `window.findRows`
- [ ] `window.waitForRows`
- [ ] `window.observeRegion`
- [ ] `window.findIconMatch`
- [ ] `window.scrollRegion`
- [ ] `window.verifyText`
- [ ] `window.clickText`
- [ ] `window.clickRow`

- [ ] **Step 4: Preserve user-visible errors**

When parsing required flags, return errors that name the invoke command and the
missing flag, for example:

```text
window.findText requires --query
```

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test -p auv-cli-invoke window -- --nocapture
cargo test -p auv-cli-invoke screen -- --nocapture
cargo test window -- --nocapture
cargo test screen -- --nocapture
```

Expected: pass.

## Task 7: Migrate Input And Overlay Commands

**Files:**
- Modify: `crates/auv-cli-invoke/src/commands/input.rs`
- Modify: `crates/auv-cli-invoke/src/commands/overlay.rs`
- Modify: `crates/auv-driver-macos/src/*`
- Test: input/overlay tests

- [ ] **Step 1: Move missing typed input/overlay APIs**

If any input/overlay capability only exists behind root string dispatch, move
it into `auv-driver-macos` or the overlay crate before invoking it.

- [ ] **Step 2: Migrate every input command handler**

Each handler should parse its own input arguments and call typed APIs directly.

- [ ] `input.focusText`
- [ ] `input.pressButton`
- [ ] `input.axPressButton`
- [ ] `input.axFocusText`
- [ ] `input.axClickWindowText`
- [ ] `input.smartPress`
- [ ] `input.typeText`
- [ ] `input.pasteText`
- [ ] `input.key`
- [ ] `input.clickPoint`
- [ ] `input.clickWindowPoint`
- [ ] `input.teachClick`
- [ ] `input.scrollPoint`

- [ ] **Step 3: Migrate every overlay command handler**

Each handler should parse its own input arguments and call typed APIs directly.

- [ ] `overlay.clickPoint`
- [ ] `overlay.showCursor`
- [ ] `overlay.showDualCursor`
- [ ] `overlay.applyCursorBatch`
- [ ] `overlay.setCursor`
- [ ] `overlay.moveCursor`
- [ ] `overlay.moveCursorById`
- [ ] `overlay.flashCursor`
- [ ] `overlay.flashCursorById`
- [ ] `overlay.hideCursorId`
- [ ] `overlay.hideCursor`
- [ ] `overlay.shutdown`

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p auv-cli-invoke input -- --nocapture
cargo test -p auv-cli-invoke overlay -- --nocapture
cargo test input -- --nocapture
cargo test overlay -- --nocapture
```

Expected: pass.

## Task 8: Remove Remaining Driver-Operation Code

**Files:**
- Modify: `crates/auv-cli-invoke/src/command.rs`
- Modify: `src/runtime.rs`
- Modify: root driver modules if now unused

- [ ] **Step 1: Verify no driver-operation calls remain**

Run:

```bash
rg "driver_operation|DriverOperation|DriverOperationRoute|invoke_driver_operation_in_span" crates/auv-cli-invoke src/runtime.rs
```

Expected: no matches.

- [ ] **Step 2: Remove dead adapter code**

Delete any remaining adapter structs, tests, and helper code that existed only
to route invoke command ids to driver operations.

- [ ] **Step 3: Remove unused root driver operations**

For root driver functions made unreachable by direct invoke migration, delete
them only when `rg` proves no other root path uses them.

## Task 9: Documentation Updates

**Files:**
- Modify:
  `docs/ai/references/2026-06-17-invoke-command-handler-binding-plan.md`
- Modify:
  `docs/ai/references/2026-06-17-auv-cli-invoke-metadata-routing-design.md`
- Create:
  `docs/ai/references/2026-06-18-invoke-direct-command-implementations-handoff.md`

- [ ] **Step 1: Mark the adapter plan as superseded**

Add:

```markdown
## Superseded Detail

`InvokeCommandExecution::DriverOperation` was an intermediate adapter and is
not the desired invoke execution model. Direct command functions now return
`InvokeCommandOutput` and call typed domain crates themselves.
```

- [ ] **Step 2: Add implementation handoff**

Create a handoff that lists:

- retained commands migrated to direct implementation
- Steam invoke command removal
- crate APIs added or moved
- root driver operations deleted
- commands, if any, intentionally deferred with owner-approved reason

## Task 10: Full Verification

**Files:**
- No code edits unless verification finds a regression.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt --check
```

Expected: pass.

- [ ] **Step 2: Invoke crate tests**

Run:

```bash
cargo test -p auv-cli-invoke -p auv-cli-invoke-macros
```

Expected: pass.

- [ ] **Step 3: Root parser and MCP tests**

Run:

```bash
cargo test parse_invoke -- --nocapture
cargo test mcp -- --nocapture
```

Expected: pass.

- [ ] **Step 4: Full suite**

Run:

```bash
cargo test
```

Expected: pass. Existing `auv-game-minecraft` deprecation warnings are allowed
if unchanged.

- [ ] **Step 5: Diff hygiene**

Run:

```bash
git diff --check
```

Expected: pass.

## Self-Review

- Spec coverage: all retained invoke command functions are covered by domain
  migration tasks, Steam invoke compatibility is removed, and the
  driver-operation adapter is deleted.
- Placeholder scan: no `TBD`; commands with missing typed APIs require moving
  the owning capability before handler migration.
- Scope check: this is a large migration. Use subagent-driven development with
  one domain per worker and review each domain before starting the next.
