# Invoke Command Handler Binding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace empty `#[invoke_command]` anchor functions with real invoke
implementation functions that are registered together with metadata.

**Architecture:** `auv-cli-invoke` remains the invoke command surface and does
not put driver ids, operation names, artifacts, signals, or verification in
macro attributes. The macro binds metadata to a function pointer; each command
function owns its implementation. For this slice, existing commands return a
driver operation execution request from inside the function body, which lets
root runtime delete its central command-id route table without making the macro
a dispatcher.

## Superseded Detail

`InvokeCommandExecution::DriverOperation` was an intermediate adapter and is not
the desired invoke execution model. The direct implementation migration replaces
it with `InvokeCommandOutput`; command functions now call typed domain crates
themselves or return an explicit typed-API gap error.

**Tech Stack:** Rust 2024, `auv-cli-invoke`, `auv-cli-invoke-macros`, root
runtime tests through `cargo test`, existing two-space `rustfmt` config.

---

## Non-Goals

- Do not reintroduce `driver`, `operation`, `artifacts`, `signals`,
  `verification`, `disturbance`, or `max_disturbance` macro attributes.
- Do not make the proc macro call drivers or generate driver routing logic.
- Do not delete `src/driver/macos` in this slice.
- Do not implement scroll scan invoke integration in this slice.
- Do not move every command into per-command files yet; the handler binding is
  the prerequisite cleanup.

## File Structure

- Modify `crates/auv-cli-invoke/src/command.rs`: add the root-agnostic handler
  function type, invoke input view, and driver operation execution enum.
- Modify `crates/auv-cli-invoke/src/lib.rs`: export the new handler types and
  update metadata tests.
- Modify `crates/auv-cli-invoke-macros/src/lib.rs`: generate
  `InvokeCommand` values that include the annotated function pointer.
- Modify `crates/auv-cli-invoke/tests/invoke_command_macro.rs`: verify a
  downstream annotated function is the registered implementation.
- Modify `crates/auv-cli-invoke/src/commands/*.rs`: replace empty anchors with
  functions returning the driver operation execution request.
- Modify `src/runtime.rs`: call the registered command implementation and
  delete the central command-id route table.
- Modify `src/cli.rs` and `src/mcp.rs`: no behavior change expected; run their
  tests to prove the registry/runtime surface still works.
- Modify
  `docs/ai/references/invoke-cli/2026-06-17-cli-invoke-metadata-routing-design.md`
  or add a short handoff note if implementation details differ from this plan.

## Core Design

Add a narrow execution binding to `auv-cli-invoke`:

```rust
type InvokeCommandHandler = fn(InvokeCommandInput<'_>) -> Result<InvokeCommandExecution, String>;

#[derive(Clone, Copy, Debug)]
pub struct InvokeCommandInput<'a> {
  pub command_id: &'a str,
  pub target_application_id: Option<&'a str>,
  pub inputs: &'a BTreeMap<String, String>,
  pub dry_run: bool,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvokeCommandExecution {
  DriverOperation {
    driver_id: &'static str,
    operation: &'static str,
  },
}
```

This enum is intentionally temporary and non-exhaustive. It is not macro
metadata; it is executable code inside each command function. The follow-up
migration can replace specific function bodies with typed domain calls or richer
outcomes command by command.

## Task 1: Add Handler Binding Types

**Files:**
- Modify: `crates/auv-cli-invoke/src/command.rs`
- Modify: `crates/auv-cli-invoke/src/lib.rs`
- Test: `crates/auv-cli-invoke/src/lib.rs`

- [ ] **Step 1: Write the failing metadata-plus-handler test**

In `crates/auv-cli-invoke/src/lib.rs`, update
`command_metadata_preserves_invoke_surface` to also execute the fixture
handler:

```rust
  #[test]
  fn command_metadata_preserves_invoke_surface() {
    let registry = default_registry();
    let command = registry
      .resolve("fixture.observe")
      .expect("fixture.observe should be registered");

    assert_eq!(command.id, "fixture.observe");
    assert_eq!(command.namespace, InvokeNamespace::Fixture);
    assert_eq!(
      command.summary,
      "Emit a deterministic observation result without touching the real UI."
    );
    assert_eq!(command.args, crate::arg::NO_ARGS);

    let execution = command
      .invoke(crate::InvokeCommandInput {
        command_id: command.id,
        target_application_id: None,
        inputs: &BTreeMap::new(),
        dry_run: false,
      })
      .expect("fixture command should produce execution");

    assert_eq!(
      execution,
      crate::InvokeCommandExecution::DriverOperation {
        driver_id: "fixture.observe",
        operation: "observe_fixture_scene",
      }
    );
  }
```

Also add `BTreeMap` to the existing test imports:

```rust
use std::collections::BTreeMap;
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
cargo test -p auv-cli-invoke command_metadata_preserves_invoke_surface -- --nocapture
```

Expected: compile failure because `InvokeCommandInput`,
`InvokeCommandExecution`, and `InvokeCommand::invoke` do not exist yet.

- [ ] **Step 3: Add the new handler types**

In `crates/auv-cli-invoke/src/command.rs`, add:

```rust
use std::collections::BTreeMap;

type InvokeCommandHandler = fn(InvokeCommandInput<'_>) -> Result<InvokeCommandExecution, String>;

#[derive(Clone, Copy, Debug)]
pub struct InvokeCommandInput<'a> {
  pub command_id: &'a str,
  pub target_application_id: Option<&'a str>,
  pub inputs: &'a BTreeMap<String, String>,
  pub dry_run: bool,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvokeCommandExecution {
  DriverOperation {
    driver_id: &'static str,
    operation: &'static str,
  },
}

impl InvokeCommandExecution {
  pub fn driver_operation(
    driver_id: &'static str,
    operation: &'static str,
  ) -> Self {
    Self::DriverOperation {
      driver_id,
      operation,
    }
  }
}
```

Change `InvokeCommand` to:

```rust
#[derive(Clone, Debug)]
pub struct InvokeCommand {
  pub id: &'static str,
  pub namespace: InvokeNamespace,
  pub summary: &'static str,
  pub args: &'static [ArgSpec],
  handler: InvokeCommandHandler,
}
```

Add:

```rust
impl InvokeCommand {
  pub fn invoke(
    &self,
    input: InvokeCommandInput<'_>,
  ) -> Result<InvokeCommandExecution, String> {
    (self.handler)(input)
  }
}
```

Change `spec` to accept the handler:

```rust
pub fn spec(
  id: &'static str,
  namespace: InvokeNamespace,
  summary: &'static str,
  args: &'static [ArgSpec],
  handler: fn(InvokeCommandInput<'_>) -> Result<InvokeCommandExecution, String>,
) -> InvokeCommand {
  InvokeCommand {
    id,
    namespace,
    summary,
    args,
    handler,
  }
}
```

- [ ] **Step 4: Export the new public types**

In `crates/auv-cli-invoke/src/lib.rs`, change the command re-export to:

```rust
pub use command::{
  CommandGroup, CommandNode, InvokeCommand, InvokeCommandExecution, InvokeCommandInput,
  InvokeNamespace,
};
```

Do not re-export `InvokeCommandHandler`; it is an internal alias for the private
field and should not become a public API name.

- [ ] **Step 5: Run the focused test**

Run:

```bash
cargo test -p auv-cli-invoke command_metadata_preserves_invoke_surface -- --nocapture
```

Expected: compile failure in the macro and command modules because `spec` now
requires a handler and empty functions return `()`.

## Task 2: Make the Macro Bind Metadata to the Function

**Files:**
- Modify: `crates/auv-cli-invoke-macros/src/lib.rs`
- Modify: `crates/auv-cli-invoke/tests/invoke_command_macro.rs`

- [ ] **Step 1: Update the downstream macro test**

Replace the annotated function in
`crates/auv-cli-invoke/tests/invoke_command_macro.rs` with:

```rust
#[invoke_command(
  id = "external.generated",
  group = "fixture",
  summary = "External generated test command.",
  args = auv_cli_invoke::arg::NO_ARGS,
)]
fn external_generated_command_handler(
  _input: auv_cli_invoke::InvokeCommandInput<'_>,
) -> Result<auv_cli_invoke::InvokeCommandExecution, String> {
  Ok(
    auv_cli_invoke::InvokeCommandExecution::driver_operation(
      "fixture.observe",
      "observe_fixture_scene",
    ),
  )
}
```

Extend the test with:

```rust
  let execution = command
    .invoke(auv_cli_invoke::InvokeCommandInput {
      command_id: command.id,
      target_application_id: None,
      inputs: &std::collections::BTreeMap::new(),
      dry_run: false,
    })
    .expect("handler should run");

  assert_eq!(
    execution,
    auv_cli_invoke::InvokeCommandExecution::DriverOperation {
      driver_id: "fixture.observe",
      operation: "observe_fixture_scene",
    }
  );
```

- [ ] **Step 2: Run the downstream macro test and verify it fails**

Run:

```bash
cargo test -p auv-cli-invoke --test invoke_command_macro -- --nocapture
```

Expected: compile failure because the macro still generates
`command::spec(id, namespace, summary, args)` without the function pointer.

- [ ] **Step 3: Update macro generation**

In `crates/auv-cli-invoke-macros/src/lib.rs`, change the generated code to:

```rust
  let generated = format!(
    "pub fn {export_name}() -> ::auv_cli_invoke::InvokeCommand {{
      ::auv_cli_invoke::command::spec(
        {id},
        ::auv_cli_invoke::InvokeNamespace::{namespace},
        {summary},
        {args},
        {function_name},
      )
    }}"
  );
```

Keep `ALLOWED_ATTR_KEYS` unchanged:

```rust
const ALLOWED_ATTR_KEYS: &[&str] = &["id", "group", "summary", "args"];
```

- [ ] **Step 4: Run the downstream macro test**

Run:

```bash
cargo test -p auv-cli-invoke --test invoke_command_macro -- --nocapture
```

Expected: pass after command modules are migrated in Task 3; before Task 3,
crate compilation may still fail because local command functions have the old
empty signature.

## Task 3: Replace Empty Command Anchors With Implementations

**Files:**
- Modify: `crates/auv-cli-invoke/src/commands/app.rs`
- Modify: `crates/auv-cli-invoke/src/commands/display.rs`
- Modify: `crates/auv-cli-invoke/src/commands/fixture.rs`
- Modify: `crates/auv-cli-invoke/src/commands/input.rs`
- Modify: `crates/auv-cli-invoke/src/commands/media_control.rs`
- Modify: `crates/auv-cli-invoke/src/commands/overlay.rs`
- Modify: `crates/auv-cli-invoke/src/commands/screen.rs`
- Modify: `crates/auv-cli-invoke/src/commands/steam.rs`
- Modify: `crates/auv-cli-invoke/src/commands/window.rs`
- Test: `crates/auv-cli-invoke/src/lib.rs`

- [ ] **Step 1: Add local imports to every command module**

Every command module with `#[invoke_command]` should import:

```rust
use crate::{InvokeCommandExecution, InvokeCommandInput};
```

Keep existing `CommandGroup`, `arg::*`, and `invoke_command` imports.

- [ ] **Step 2: Migrate `fixture.observe` first**

In `crates/auv-cli-invoke/src/commands/fixture.rs`, replace:

```rust
fn observe() {}
```

with:

```rust
fn observe(
  _input: InvokeCommandInput<'_>,
) -> Result<InvokeCommandExecution, String> {
  Ok(InvokeCommandExecution::driver_operation(
    "fixture.observe",
    "observe_fixture_scene",
  ))
}
```

- [ ] **Step 3: Run the focused fixture test**

Run:

```bash
cargo test -p auv-cli-invoke command_metadata_preserves_invoke_surface -- --nocapture
```

Expected: compile failure from other command modules still using empty
functions.

- [ ] **Step 4: Migrate all remaining command functions**

Use this mapping for function bodies. Each body should be:

```rust
fn function_name(
  _input: InvokeCommandInput<'_>,
) -> Result<InvokeCommandExecution, String> {
  Ok(InvokeCommandExecution::driver_operation(
    "DRIVER_ID",
    "OPERATION",
  ))
}
```

Mappings:

```text
app.activate -> macos.desktop activate_app
app.probePermissions -> macos.desktop probe_permissions
display.capture -> macos.desktop capture_display
display.identifyPoint -> macos.desktop identify_point
display.list -> macos.desktop list_displays
display.probeCoordinateReadiness -> macos.desktop probe_coordinate_readiness
display.projectScreenshotPoint -> macos.desktop project_screenshot_point
fixture.observe -> fixture.observe observe_fixture_scene
input.axClickWindowText -> macos.desktop ax_click_window_text
input.axFocusText -> macos.desktop ax_focus_text_input
input.axPressButton -> macos.desktop ax_press_button
input.clickPoint -> macos.desktop click_point
input.clickWindowPoint -> macos.desktop click_window_point
input.focusText -> macos.desktop focus_text_input
input.key -> macos.desktop press_key
input.pasteText -> macos.desktop paste_text_preserve_clipboard
input.pressButton -> macos.desktop press_button
input.scrollPoint -> macos.desktop scroll_point
input.smartPress -> macos.desktop smart_press
input.teachClick -> macos.desktop teach_click
input.typeText -> macos.desktop type_text
mediaControl.next -> macos.desktop media_control_next
mediaControl.nowPlaying -> macos.desktop media_control_now_playing
mediaControl.pause -> macos.desktop media_control_pause
mediaControl.play -> macos.desktop media_control_play
mediaControl.previous -> macos.desktop media_control_previous
mediaControl.togglePlayPause -> macos.desktop media_control_toggle_play_pause
overlay.applyCursorBatch -> macos.desktop overlay_apply_cursor_batch
overlay.clickPoint -> macos.desktop overlay_click_point
overlay.flashCursor -> macos.desktop overlay_flash_cursor
overlay.flashCursorById -> macos.desktop overlay_flash_cursor_by_id
overlay.hideCursor -> macos.desktop overlay_hide_cursor
overlay.hideCursorId -> macos.desktop overlay_hide_cursor_id
overlay.moveCursor -> macos.desktop overlay_move_cursor
overlay.moveCursorById -> macos.desktop overlay_move_cursor_by_id
overlay.setCursor -> macos.desktop overlay_set_cursor
overlay.showCursor -> macos.desktop overlay_show_cursor
overlay.showDualCursor -> macos.desktop overlay_show_dual_cursor
overlay.shutdown -> macos.desktop overlay_shutdown
screen.captureRegion -> macos.desktop capture_region
screen.clickRow -> macos.desktop click_screen_row
screen.clickText -> macos.desktop click_screen_text
screen.findImageText -> macos.desktop find_image_text
screen.findRows -> macos.desktop find_screen_rows
screen.findText -> macos.desktop find_screen_text
screen.waitForRows -> macos.desktop wait_for_screen_rows
screen.waitForText -> macos.desktop wait_for_screen_text
steam.library.list.v0 -> steam.local steam_library_list
window.capture -> macos.desktop capture_window
window.captureAxTree -> macos.desktop capture_ax_tree
window.clickRow -> macos.desktop click_window_row
window.clickText -> macos.desktop click_window_text
window.findIconMatch -> macos.desktop find_icon_match
window.findRows -> macos.desktop find_window_rows
window.findText -> macos.desktop find_window_text
window.list -> macos.desktop list_windows
window.observeRegion -> macos.desktop observe_window_region
window.scrollRegion -> macos.desktop scroll_window_region
window.verifyText -> macos.desktop verify_ax_text
window.waitForRows -> macos.desktop wait_for_window_rows
window.waitForText -> macos.desktop wait_for_window_text
```

- [ ] **Step 5: Run auv-cli-invoke tests**

Run:

```bash
cargo test -p auv-cli-invoke -p auv-cli-invoke-macros
```

Expected: all tests pass.

## Task 4: Move Runtime From Central Route Table to Registered Handler

**Files:**
- Modify: `src/runtime.rs`
- Test: `src/runtime.rs`

- [ ] **Step 1: Add a runtime test that proves handlers drive execution**

Add this test near the existing invoke tests in `src/runtime.rs`:

```rust
  #[test]
  fn invoke_resolved_uses_registered_command_handler() {
    let runtime = Runtime::new(
      DriverRegistry::new().register(Box::new(SuccessDriver)),
      temp_store("invoke_resolved_uses_registered_command_handler"),
    );
    let registry = auv_cli_invoke::default_registry();
    let command = registry
      .resolve(TEST_COMMAND_ID)
      .expect("fixture command should exist");

    let result = runtime
      .invoke_resolved(
        InvokeRequest {
          command_id: TEST_COMMAND_ID.to_string(),
          ..InvokeRequest::default()
        },
        command,
      )
      .expect("fixture command should invoke");

    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.output_summary, "fixture observed");
  }
```

If `SuccessDriver` already produces a different summary in this file, use the
existing expected summary from `invoke_resolved_executes_fixture_observe_command`.

- [ ] **Step 2: Run the focused runtime test and verify current behavior**

Run:

```bash
cargo test invoke_resolved_uses_registered_command_handler -- --nocapture
```

Expected before implementation: the test may pass accidentally through the
central command-id route table; keep it because the next steps delete that
table.

- [ ] **Step 3: Replace `invoke_metadata_command_in_span` routing**

In `src/runtime.rs`, change:

```rust
    let route = command_id_route(command.id)
      .ok_or_else(|| format!("command {} has no runtime route", command.id))?;
    self.invoke_driver_operation_in_span(run, parent, command.id, route, request)
```

to:

```rust
    let execution = command
      .invoke(auv_cli_invoke::InvokeCommandInput {
        command_id: command.id,
        target_application_id: request.target.application_id.as_deref(),
        inputs: &request.inputs,
        dry_run: request.dry_run,
      })
      .map_err(|error| format!("command {} failed before runtime execution: {error}", command.id))?;

    match execution {
      auv_cli_invoke::InvokeCommandExecution::DriverOperation {
        driver_id,
        operation,
      } => self.invoke_driver_operation_in_span(
        run,
        parent,
        command.id,
        DriverOperationRoute {
          driver_id,
          operation,
        },
        request,
      ),
    }
```

- [ ] **Step 4: Delete the central route table**

Delete the central command-id route function. Keep `DriverOperationRoute` as the
small private runtime adapter struct used by `invoke_driver_operation_in_span`.

Delete the old runtime test:

```rust
fn default_registry_commands_have_driver_operation_handlers()
```

Replace it with:

```rust
  #[test]
  fn default_registry_commands_have_registered_handlers() {
    let registry = auv_cli_invoke::default_registry();

    for command in registry.all() {
      let execution = command
        .invoke(auv_cli_invoke::InvokeCommandInput {
          command_id: command.id,
          target_application_id: None,
          inputs: &BTreeMap::new(),
          dry_run: false,
        })
        .unwrap_or_else(|error| panic!("{} handler should resolve: {error}", command.id));

      match execution {
        auv_cli_invoke::InvokeCommandExecution::DriverOperation {
          driver_id,
          operation,
        } => {
          assert!(!driver_id.is_empty(), "{} should name a driver", command.id);
          assert!(!operation.is_empty(), "{} should name an operation", command.id);
        }
      }
    }
  }
```

- [ ] **Step 5: Run runtime tests**

Run:

```bash
cargo test invoke_resolved_uses_registered_command_handler -- --nocapture
cargo test default_registry_commands_have_registered_handlers -- --nocapture
```

Expected: both pass.

## Task 5: Remove Stale Anchor Language From Docs and Help Tests

**Files:**
- Modify:
  `docs/ai/references/invoke-cli/2026-06-17-cli-invoke-metadata-routing-design.md`
- Modify:
  `docs/ai/references/invoke-cli/2026-06-17-cli-invoke-routing-implementation-plan.md`
- Test: `crates/auv-cli-invoke/src/lib.rs`

- [ ] **Step 1: Update the design note**

Replace language that says the annotated function is only an anchor with:

```markdown
`#[invoke_command]` marks an invoke implementation function as routable from the
invoke sub-command registry. The macro standardizes metadata and registers the
function pointer; the function body owns execution.
```

Add:

```markdown
The temporary `DriverOperation` return value is an implementation detail
inside command functions. It is not macro metadata and should be replaced
command-by-command as typed domain capabilities become available.
```

- [ ] **Step 2: Update the implementation handoff**

Add a short section:

```markdown
## Follow-Up: Handler Binding

The metadata-only refactor intentionally removed macro-owned driver dispatch.
The next slice binds each metadata entry to its implementation function so empty
macro anchors disappear while driver routing remains outside macro attributes.
```

- [ ] **Step 3: Run doc-adjacent tests**

Run:

```bash
cargo test -p auv-cli-invoke command_help_renders_metadata_only_sections -- --nocapture
```

Expected: pass. Help should still omit `DRIVER`, `DISTURBANCE`, `ARTIFACTS`,
`SIGNALS`, and `VERIFY`.

## Task 6: Full Verification

**Files:**
- No code edits unless verification finds a regression.

- [ ] **Step 1: Format check**

Run:

```bash
cargo fmt --check
```

Expected: pass.

- [ ] **Step 2: Invoke crates tests**

Run:

```bash
cargo test -p auv-cli-invoke -p auv-cli-invoke-macros
```

Expected: pass.

- [ ] **Step 3: Parser tests**

Run:

```bash
cargo test parse_invoke -- --nocapture
```

Expected: pass, including
`parse_invoke_preserves_inspect_like_command_input_values`.

- [ ] **Step 4: MCP tests**

Run:

```bash
cargo test mcp -- --nocapture
```

Expected: pass.

- [ ] **Step 5: Full test suite**

Run:

```bash
cargo test
```

Expected: pass. Existing `auv-game-minecraft` deprecation warnings are allowed
if they remain unchanged.

- [ ] **Step 6: Diff hygiene**

Run:

```bash
git diff --check
```

Expected: pass.

## Self-Review

- Spec coverage: the plan removes empty anchor functions, binds metadata to
  real implementation functions, keeps macro attributes metadata-only, deletes
  the root central route table, and preserves current invoke execution.
- Placeholder scan: no `TBD`, no unspecified error handling, and no task says
  “write tests” without concrete test code.
- Type consistency: `InvokeCommandInput`, `InvokeCommandExecution`,
  `InvokeCommandHandler`, and `InvokeCommand::invoke` are defined in Task 1 and
  used consistently in later tasks.
