# auv-cli-invoke Handler-First Commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace string-only `spec(...)` command declarations with handler-first `#[invoke_command]` declarations that generate `as_invoke_command`-style exports.

**Architecture:** Add a small proc-macro crate that binds invoke metadata to a real handler function. `auv-cli-invoke` remains the command registry owner; each domain module registers generated command exports, while runtime dispatch uses each command's handler to build the driver dispatch request.

**Tech Stack:** Rust 2024, `proc_macro` without `syn`/`quote` for the first slice, existing `auv-cli-invoke`, existing `auv-driver::OperationSpec`, existing root runtime tests.

---

## File Structure

- Create `crates/auv-cli-invoke-macros/Cargo.toml`: proc-macro crate manifest.
- Create `crates/auv-cli-invoke-macros/src/lib.rs`: `#[invoke_command]` parser and code generator.
- Modify `Cargo.toml`: add `crates/auv-cli-invoke-macros` to workspace members.
- Modify `crates/auv-cli-invoke/Cargo.toml`: depend on `auv-cli-invoke-macros`.
- Modify `crates/auv-cli-invoke/src/command.rs`: add handler types, dispatch structs, and public helper functions used by generated commands.
- Modify `crates/auv-cli-invoke/src/lib.rs`: re-export `invoke_command`, handler types, and add tests for macro-generated commands.
- Modify `crates/auv-cli-invoke/src/commands/*.rs`: replace `spec(...)` calls with annotated handler functions plus generated `*_invoke_command()` registrations.
- Modify `src/runtime.rs`: use the resolved command handler to build driver dispatch instead of manually copying operation strings from metadata.
- Modify `docs/ai/references/invoke-cli/2026-06-11-cli-invoke-driver-console-design.md`: mark handler-first implementation landed and describe remaining typed-args follow-up.

## Task 1: Add Macro Crate Skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/auv-cli-invoke-macros/Cargo.toml`
- Create: `crates/auv-cli-invoke-macros/src/lib.rs`
- Modify: `crates/auv-cli-invoke/Cargo.toml`

- [ ] **Step 1: Add the workspace member**

Add `"crates/auv-cli-invoke-macros"` to the `[workspace].members` array in `Cargo.toml`, directly before `"crates/auv-cli-invoke"`.

- [ ] **Step 2: Add the proc-macro manifest**

Create `crates/auv-cli-invoke-macros/Cargo.toml` with:

```toml
[package]
name = "auv-cli-invoke-macros"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true
readme.workspace = true
license.workspace = true
authors.workspace = true
description.workspace = true
documentation.workspace = true
homepage.workspace = true
repository.workspace = true

[lib]
proc-macro = true
```

- [ ] **Step 3: Add a compiling placeholder macro**

Create `crates/auv-cli-invoke-macros/src/lib.rs` with:

```rust
use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn invoke_command(_attr: TokenStream, item: TokenStream) -> TokenStream {
  item
}
```

- [ ] **Step 4: Wire the macro crate into `auv-cli-invoke`**

Add this dependency to `crates/auv-cli-invoke/Cargo.toml`:

```toml
auv-cli-invoke-macros = { path = "../auv-cli-invoke-macros" }
```

- [ ] **Step 5: Verify the skeleton builds**

Run:

```bash
cargo test -p auv-cli-invoke
```

Expected: existing `auv-cli-invoke` tests pass.

## Task 2: Add Handler Dispatch Types

**Files:**
- Modify: `crates/auv-cli-invoke/src/command.rs`
- Modify: `crates/auv-cli-invoke/src/lib.rs`
- Modify: `crates/auv-cli-invoke/src/registry.rs`

- [ ] **Step 1: Add a failing handler dispatch test**

In `crates/auv-cli-invoke/src/lib.rs`, add:

```rust
#[test]
fn command_handler_builds_default_driver_dispatch() {
  let registry = default_registry();
  let command = registry
    .resolve("fixture.observe")
    .expect("fixture.observe should be registered");
  let mut inputs = std::collections::BTreeMap::new();
  inputs.insert("scene".to_string(), "demo".to_string());
  let context = InvokeContext {
    target_application_id: Some("com.example.App"),
    target_label: Some("Example"),
    inputs: &inputs,
  };

  let dispatch = command.dispatch(context);

  assert_eq!(dispatch.command_id, "fixture.observe");
  assert_eq!(dispatch.driver_id, "fixture.observe");
  assert_eq!(dispatch.operation, "observe_fixture_scene");
  assert_eq!(dispatch.target_application_id.as_deref(), Some("com.example.App"));
  assert_eq!(dispatch.target_label.as_deref(), Some("Example"));
  assert_eq!(dispatch.inputs.get("scene").map(String::as_str), Some("demo"));
}
```

Run:

```bash
cargo test -p auv-cli-invoke command_handler_builds_default_driver_dispatch
```

Expected: fail because `InvokeContext`, `dispatch`, and dispatch fields do not exist yet.

- [ ] **Step 2: Add handler types and default dispatch**

In `crates/auv-cli-invoke/src/command.rs`, add:

```rust
use std::collections::BTreeMap;
```

Add these types near `InvokeCommand`:

```rust
#[derive(Clone, Copy, Debug)]
pub struct InvokeContext<'a> {
  pub target_application_id: Option<&'a str>,
  pub target_label: Option<&'a str>,
  pub inputs: &'a BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeDriverDispatch {
  pub command_id: &'static str,
  pub driver_id: &'static str,
  pub operation: &'static str,
  pub target_application_id: Option<String>,
  pub target_label: Option<String>,
  pub inputs: BTreeMap<String, String>,
}

pub type InvokeCommandHandler = fn(InvokeContext<'_>, &InvokeCommand) -> InvokeDriverDispatch;
```

Add fields to `InvokeCommand`:

```rust
pub handler: InvokeCommandHandler,
pub handler_name: &'static str,
```

Add methods and default handler:

```rust
impl InvokeCommand {
  pub fn dispatch(&self, context: InvokeContext<'_>) -> InvokeDriverDispatch {
    (self.handler)(context, self)
  }
}

pub fn default_driver_dispatch(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  InvokeDriverDispatch {
    command_id: command.operation.id,
    driver_id: command.operation.driver_id,
    operation: command.operation.operation,
    target_application_id: context.target_application_id.map(str::to_string),
    target_label: context.target_label.map(str::to_string),
    inputs: context.inputs.clone(),
  }
}
```

In `spec_with_operation_namespace`, initialize:

```rust
handler: default_driver_dispatch,
handler_name: "default_driver_dispatch",
```

- [ ] **Step 3: Re-export handler types**

In `crates/auv-cli-invoke/src/lib.rs`, re-export:

```rust
pub use command::{
  CommandGroup, CommandNode, InvokeCommand, InvokeCommandHandler, InvokeContext,
  InvokeDriverDispatch, InvokeNamespace, default_driver_dispatch,
};
pub use auv_cli_invoke_macros::invoke_command;
```

- [ ] **Step 4: Verify handler dispatch passes**

Run:

```bash
cargo test -p auv-cli-invoke command_handler_builds_default_driver_dispatch
```

Expected: pass.

## Task 3: Implement `#[invoke_command]`

**Files:**
- Modify: `crates/auv-cli-invoke-macros/src/lib.rs`
- Modify: `crates/auv-cli-invoke/src/lib.rs`

- [ ] **Step 1: Add a failing macro generation test**

In `crates/auv-cli-invoke/src/lib.rs`, add:

```rust
#[invoke_command(
  id = "test.generated",
  group = "fixture",
  summary = "Generated test command.",
  driver = "fixture.observe",
  operation = "observe_fixture_scene",
  args = crate::arg::NO_ARGS,
  disturbance = crate::command::NONE,
  max_disturbance = auv_driver::OperationDisturbance::None,
  artifacts = ["operation-result"],
  signals = ["fixture.scene"],
  verification = "read-only; no semantic success claim",
)]
fn generated_test_command_handler(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[test]
fn invoke_command_macro_generates_export() {
  let command = generated_test_command_handler_invoke_command();

  assert_eq!(command.operation.id, "test.generated");
  assert_eq!(command.namespace, InvokeNamespace::Fixture);
  assert_eq!(command.handler_name, "generated_test_command_handler");
  assert_eq!(command.artifacts, &["operation-result"]);
  assert_eq!(command.signals, &["fixture.scene"]);
}
```

Run:

```bash
cargo test -p auv-cli-invoke invoke_command_macro_generates_export
```

Expected: fail because the placeholder macro does not generate `generated_test_command_handler_invoke_command`.

- [ ] **Step 2: Implement the attribute parser and code generation**

In `crates/auv-cli-invoke-macros/src/lib.rs`, replace the placeholder macro with code that:

- extracts the annotated function name by scanning token trees for `fn` followed by an identifier
- parses top-level `key = value` pairs from the attribute token string
- requires these keys: `id`, `group`, `summary`, `driver`, `operation`, `args`, `disturbance`, `max_disturbance`, `artifacts`, `signals`, `verification`
- accepts optional `operation_namespace`
- generates the original function plus:

```rust
pub fn <handler_name>_invoke_command() -> ::auv_cli_invoke::InvokeCommand {
  ::auv_cli_invoke::command::spec(
    <id>,
    ::auv_cli_invoke::InvokeNamespace::<GroupVariant>,
    <summary>,
    <driver>,
    <operation>,
    <disturbance>,
    <max_disturbance>,
    <args>,
    &<artifacts>,
    &<signals>,
    <verification>,
  )
  .with_handler(<handler_name>, stringify!(<handler_name>))
}
```

If `operation_namespace` is present, generate `spec_with_operation_namespace` and pass the provided namespace expression before `summary`.

The macro should validate `group` during expansion and emit `compile_error!`
for unknown groups. Do not defer group typos to registry-construction panics.

- [ ] **Step 3: Add command helpers needed by generated code**

In `crates/auv-cli-invoke/src/command.rs`, add:

```rust
impl InvokeCommand {
  pub fn with_handler(
    mut self,
    handler: InvokeCommandHandler,
    handler_name: &'static str,
  ) -> Self {
    self.handler = handler;
    self.handler_name = handler_name;
    self
  }
}
```

Make `spec` and `spec_with_operation_namespace` hidden public helpers because
the re-exported macro expands in downstream crate contexts:

```rust
#[doc(hidden)]
pub fn spec(...)

#[doc(hidden)]
pub fn spec_with_operation_namespace(...)
```

Add `extern crate self as auv_cli_invoke;` in `auv-cli-invoke` so the same
absolute path works inside the crate and from downstream crates.

- [ ] **Step 4: Verify macro generation**

Run:

```bash
cargo test -p auv-cli-invoke invoke_command_macro_generates_export
```

Expected: pass.

## Task 4: Migrate One Domain And Runtime Dispatch

**Files:**
- Modify: `crates/auv-cli-invoke/src/commands/fixture.rs`
- Modify: `crates/auv-cli-invoke/src/registry.rs`
- Modify: `src/runtime.rs`

- [ ] **Step 1: Migrate `fixture.observe` to the macro**

Replace the `spec(...)` command declaration in `crates/auv-cli-invoke/src/commands/fixture.rs` with:

```rust
use auv_driver::OperationDisturbance;

use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch, default_driver_dispatch,
  invoke_command,
  arg::NO_ARGS,
  command::NONE,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("fixture", "FIXTURE").command(observe_invoke_command())
}

#[invoke_command(
  id = "fixture.observe",
  group = "fixture",
  summary = "Emit a deterministic observation result without touching the real UI.",
  driver = "fixture.observe",
  operation = "observe_fixture_scene",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["operation-result"],
  signals = ["fixture.scene"],
  verification = "read-only; no semantic success claim",
)]
pub fn observe(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
```

- [ ] **Step 2: Add a failing test for handler identity**

In `crates/auv-cli-invoke/src/lib.rs`, add:

```rust
#[test]
fn fixture_command_uses_named_handler() {
  let registry = default_registry();
  let command = registry
    .resolve("fixture.observe")
    .expect("fixture.observe should be registered");

  assert_eq!(command.handler_name, "observe");
}
```

Run:

```bash
cargo test -p auv-cli-invoke fixture_command_uses_named_handler
```

Expected: pass after Step 1.

- [ ] **Step 3: Route runtime through command handler**

In `src/runtime.rs`, update `invoke_in_span` to construct an `auv_cli_invoke::InvokeContext` from `InvokeRequest` and call `command.dispatch(context)`. Use the returned dispatch to build `DriverCall`.

The resulting driver lookup should use:

```rust
let dispatch = command.dispatch(auv_cli_invoke::InvokeContext {
  target_application_id: request.target.application_id.as_deref(),
  target_label: request.target.label.as_deref(),
  inputs: &request.inputs,
});
```

Then `invoke_direct_command_in_span` should receive both `&command.operation` for trace metadata and `dispatch` for driver call construction.

- [ ] **Step 4: Verify runtime fixture invoke**

Run:

```bash
cargo test runtime::tests::invoke_resolved_executes_fixture_observe_command
```

Expected: pass.

## Task 5: Migrate All Built-In Commands

**Files:**
- Modify all files under `crates/auv-cli-invoke/src/commands/*.rs`

- [ ] **Step 1: Convert each `spec(...)` call to an annotated handler**

For every existing command declaration, create one handler function named after the driver operation, for example:

```rust
#[invoke_command(
  id = "screen.captureRegion",
  group = "screen",
  summary = "Capture one display-contained region and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
  driver = "macos.desktop",
  operation = "capture_region",
  args = REGION_ARGS,
  disturbance = NONE_OR_FOREGROUND,
  max_disturbance = OperationDisturbance::ForegroundApp,
  artifacts = ["region-capture", "capture-contract"],
  signals = [],
  verification = "capture-only; no semantic success claim",
)]
pub fn capture_region(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
```

Then update each `group()` function to register the generated export:

```rust
pub fn group() -> CommandGroup {
  CommandGroup::new("screen", "SCREEN")
    .command(capture_region_invoke_command())
    .command(find_screen_text_invoke_command())
}
```

For commands that currently use `spec_with_operation_namespace`, include:

```rust
operation_namespace = OperationNamespace::Action
```

or:

```rust
operation_namespace = OperationNamespace::Verify
```

- [ ] **Step 2: Remove unused `spec` imports from command modules**

Every migrated command module should import:

```rust
use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch, default_driver_dispatch,
  invoke_command,
  ...
};
```

and should no longer import `spec` or `spec_with_operation_namespace`.

- [ ] **Step 3: Add an all-commands handler-name test**

In `crates/auv-cli-invoke/src/lib.rs`, add:

```rust
#[test]
fn all_default_commands_have_named_handlers() {
  let registry = default_registry();

  for command in registry.all() {
    assert_ne!(
      command.handler_name, "default_driver_dispatch",
      "{} should be declared through #[invoke_command]",
      command.operation.id
    );
  }
}
```

- [ ] **Step 4: Verify command metadata stayed stable**

Run:

```bash
cargo test -p auv-cli-invoke
cargo run --quiet -- invoke --help
cargo run --quiet -- invoke window.capture --help
cargo run --quiet -- invoke mediaControl.nowPlaying --help
```

Expected:

- `auv-cli-invoke` tests pass.
- Help still lists the same command ids.
- Command-specific help still shows the same driver, options, artifacts, signals, and verification text.

## Task 6: Documentation And Final Verification

**Files:**
- Modify: `docs/ai/references/invoke-cli/2026-06-11-cli-invoke-driver-console-design.md`
- Modify: `docs/TERMS_AND_CONCEPTS.md` if the handler-first command boundary text needs one sentence of durable vocabulary.

- [ ] **Step 1: Update the invoke design reference**

In `docs/ai/references/invoke-cli/2026-06-11-cli-invoke-driver-console-design.md`, change the handler-first target section from future tense to landed state for:

- `#[invoke_command]`
- generated `*_invoke_command()` export
- command handler traceability
- typed args remaining as a follow-up

- [ ] **Step 2: Run final checks**

Run:

```bash
cargo fmt --check
cargo test -p auv-cli-invoke
cargo test runtime::tests::invoke_resolved_executes_fixture_observe_command
cargo test cli::tests
cargo run --quiet -- invoke --help
git diff --check
```

Expected: all commands pass. If `cargo clippy --all-targets --all-features` is run, it may still report existing warnings outside this slice, but it must exit 0.

- [ ] **Step 3: Commit**

Commit with:

```bash
git add Cargo.toml crates/auv-cli-invoke crates/auv-cli-invoke-macros src/runtime.rs docs/ai/references/invoke-cli/2026-06-11-cli-invoke-driver-console-design.md docs/ai/references/invoke-cli/2026-06-13-cli-invoke-handler-first-plan.md docs/TERMS_AND_CONCEPTS.md
git commit -m "refactor(invoke): declare commands from handlers"
```

## Self-Review

- Spec coverage: the plan implements handler-first command declarations, generated exports, runtime handler dispatch, and docs for remaining typed args.
- Placeholder scan: no `TBD` or open-ended implementation steps remain; each task names concrete files and commands.
- Type consistency: `InvokeContext`, `InvokeDriverDispatch`, `InvokeCommandHandler`, `default_driver_dispatch`, `with_handler`, and generated `*_invoke_command()` are named consistently across tasks.
