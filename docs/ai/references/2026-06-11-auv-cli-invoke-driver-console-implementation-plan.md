# auv-cli-invoke Driver Console Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the root command catalog with an `invoke` driver console boundary that owns command metadata, `invoke --help`, command-specific help, new camelCase capability ids, and generic `mediaControl.*` commands.

**Architecture:** Task 9 supersedes the initial Tasks 1-8 root-module
approach. The final PR shape is a dedicated `crates/auv-cli-invoke` crate for
invoke command registration/help and `auv_driver::operation` for driver
operation metadata. `Runtime` executes `auv_driver::OperationSpec` values and
no longer owns command discovery. The legacy macOS string-operation driver
adapter remains the execution bridge for this PR.

**Tech Stack:** Rust 2024, existing root `auv-cli` crate, existing `DriverCall`/`DriverResponse` adapter, `auv-media-macos` for media session commands, standard Rust unit tests, Cargo verification.

---

## Source Spec

Implement against:

- `docs/ai/references/2026-06-11-auv-cli-invoke-driver-console-design.md`

## Initial File Structure (Tasks 1-8, Superseded By Task 9)

- Create `src/invoke.rs`
  - Owns `InvokeCommandSpec`, `InvokeArgSpec`, `InvokeNamespace`, `InvokeRegistry`, default registry construction, command lookup, and help rendering.
  - Contains an explicit module-level `NOTICE:` that crate extraction is deferred until the registry no longer depends on root runtime models.
- Modify `src/lib.rs`
  - Export `pub mod invoke`.
  - Stop exporting `pub mod catalog`.
  - Build `Runtime` without a command catalog.
- Delete `src/catalog.rs`
  - Move useful command metadata into `src/invoke.rs`.
- Modify `src/runtime.rs`
  - Remove `CommandCatalog` storage and `list_commands`.
  - Add `invoke_resolved(request, command)`.
  - Keep run recording and driver execution behavior unchanged.
- Modify `src/cli.rs`
  - Remove `CliCommand::ListCommands`.
  - Add invoke help parse variants.
  - Parse `auv-cli invoke --help` and `auv-cli invoke <command> --help`.
  - Keep generic flag-to-input parsing for actual invoke execution.
- Modify `src/main.rs`
  - Render invoke help without building a runtime.
  - Resolve invoke command through `invoke::default_registry()` before calling runtime.
  - Make `list-commands` fail with guidance to `invoke --help`.
- Modify `src/driver/macos/dispatch.rs`
  - Remove generic invoke dispatch exposure for app/domain-specific `music_*` and `recognition_read_ratio` operations.
  - Add dispatch for generic media control operations.
- Create `src/driver/macos/media_control.rs`
  - Bridge `auv-media-macos` into `DriverResponse`.
  - Implement `media_control_now_playing`, `media_control_play`, `media_control_pause`, `media_control_toggle_play_pause`, `media_control_next`, and `media_control_previous`.
- Modify `src/driver/macos/mod.rs`
  - Add `mod media_control`.
- Modify root `Cargo.toml`
  - Add `auv-media-macos = { path = "crates/auv-media-macos" }` under macOS target dependencies.
- Update tests in `src/cli.rs`, `src/runtime.rs`, `src/app/tests.rs`, and
  `src/scroll_scan/mod.rs`.
- Before deleting `src/catalog.rs`, run `rg "crate::catalog::CommandCatalog|default_command_catalog|mod catalog|pub mod catalog" src -g '*.rs'` and remove every remaining import or module reference found by that command.

## Rename And Removal Map

Register these new invoke ids and keep their existing driver operations:

| Old id | New id | Operation |
| --- | --- | --- |
| `debug.captureDisplay` | `display.capture` | `capture_display` |
| `debug.captureRegion` | `screen.captureRegion` | `capture_region` |
| `debug.captureWindow` | `window.capture` | `capture_window` |
| `debug.listDisplays` | `display.list` | `list_displays` |
| `debug.projectScreenshotPoint` | `display.projectScreenshotPoint` | `project_screenshot_point` |
| `debug.identifyPoint` | `display.identifyPoint` | `identify_point` |
| `debug.probeCoordinateReadiness` | `display.probeCoordinateReadiness` | `probe_coordinate_readiness` |
| `debug.findScreenText` | `screen.findText` | `find_screen_text` |
| `debug.waitForScreenText` | `screen.waitForText` | `wait_for_screen_text` |
| `debug.findScreenRows` | `screen.findRows` | `find_screen_rows` |
| `debug.waitForScreenRows` | `screen.waitForRows` | `wait_for_screen_rows` |
| `debug.findImageText` | `screen.findImageText` | `find_image_text` |
| `debug.findWindowText` | `window.findText` | `find_window_text` |
| `debug.waitForWindowText` | `window.waitForText` | `wait_for_window_text` |
| `debug.findWindowRows` | `window.findRows` | `find_window_rows` |
| `debug.waitForWindowRows` | `window.waitForRows` | `wait_for_window_rows` |
| `debug.observeWindowRegion` | `window.observeRegion` | `observe_window_region` |
| `debug.findIconMatch` | `window.findIconMatch` | `find_icon_match` |
| `debug.scrollWindowRegion` | `window.scrollRegion` | `scroll_window_region` |
| `verify.axText` | `window.verifyText` | `verify_ax_text` |
| `debug.listWindows` | `window.list` | `list_windows` |
| `debug.captureAxTree` | `window.captureAxTree` | `capture_ax_tree` |
| `debug.probePermissions` | `app.probePermissions` | `probe_permissions` |
| `debug.activateApp` | `app.activate` | `activate_app` |
| `debug.focusTextInput` | `input.focusText` | `focus_text_input` |
| `debug.pressButton` | `input.pressButton` | `press_button` |
| `debug.axPressButton` | `input.axPressButton` | `ax_press_button` |
| `debug.axFocusTextInput` | `input.axFocusText` | `ax_focus_text_input` |
| `debug.axClickWindowText` | `input.axClickWindowText` | `ax_click_window_text` |
| `debug.smartPress` | `input.smartPress` | `smart_press` |
| `debug.typeText` | `input.typeText` | `type_text` |
| `debug.pasteTextPreserveClipboard` | `input.pasteText` | `paste_text_preserve_clipboard` |
| `debug.pressKey` | `input.key` | `press_key` |
| `debug.clickPoint` | `input.clickPoint` | `click_point` |
| `debug.clickWindowPoint` | `input.clickWindowPoint` | `click_window_point` |
| `debug.teachClick` | `input.teachClick` | `teach_click` |
| `debug.clickScreenText` | `screen.clickText` | `click_screen_text` |
| `debug.clickScreenRow` | `screen.clickRow` | `click_screen_row` |
| `debug.clickWindowText` | `window.clickText` | `click_window_text` |
| `debug.clickWindowRow` | `window.clickRow` | `click_window_row` |
| `debug.scrollPoint` | `input.scrollPoint` | `scroll_point` |
| `debug.overlayClickPoint` | `overlay.clickPoint` | `overlay_click_point` |
| `debug.overlayShowCursor` | `overlay.showCursor` | `overlay_show_cursor` |
| `debug.overlayShowDualCursor` | `overlay.showDualCursor` | `overlay_show_dual_cursor` |
| `debug.overlayApplyCursorBatch` | `overlay.applyCursorBatch` | `overlay_apply_cursor_batch` |
| `debug.overlaySetCursor` | `overlay.setCursor` | `overlay_set_cursor` |
| `debug.overlayMoveCursor` | `overlay.moveCursor` | `overlay_move_cursor` |
| `debug.overlayMoveCursorById` | `overlay.moveCursorById` | `overlay_move_cursor_by_id` |
| `debug.overlayFlashCursor` | `overlay.flashCursor` | `overlay_flash_cursor` |
| `debug.overlayFlashCursorById` | `overlay.flashCursorById` | `overlay_flash_cursor_by_id` |
| `debug.overlayHideCursorId` | `overlay.hideCursorId` | `overlay_hide_cursor_id` |
| `debug.overlayHideCursor` | `overlay.hideCursor` | `overlay_hide_cursor` |
| `debug.overlayShutdown` | `overlay.shutdown` | `overlay_shutdown` |
| `debug.fixtureObserve` | `fixture.observe` | `observe_fixture_scene` |

Register these new media ids:

| New id | Operation |
| --- | --- |
| `mediaControl.nowPlaying` | `media_control_now_playing` |
| `mediaControl.play` | `media_control_play` |
| `mediaControl.pause` | `media_control_pause` |
| `mediaControl.togglePlayPause` | `media_control_toggle_play_pause` |
| `mediaControl.next` | `media_control_next` |
| `mediaControl.previous` | `media_control_previous` |

Do not register these old or app/domain-specific ids:

- `debug.*`
- `verify.*`
- `music.validate.candidate.liveness`
- `music.search.results`
- `music.result.play`
- `recognition.read.ratio`
- `steam.library.list.v0`

---

### Task 1: Add Invoke Metadata And Help Renderer

**Files:**
- Create: `src/invoke.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing help-rendering tests in `src/invoke.rs`**

Add the file with tests first. Use this exact skeleton:

```rust
// File: src/invoke.rs
//! Invoke command metadata and help rendering.
//!
//! NOTICE: This starts as a root module instead of a workspace crate because
//! command specs still reuse root runtime model types. Extract to an
//! `auv-cli-invoke` crate once those types no longer depend on root runtime
//! internals.

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn help_index_groups_commands_by_namespace() {
    let registry = default_registry();

    let help = render_help_index(&registry);

    assert!(help.contains("USAGE\n  auv-cli invoke <command> [options]"));
    assert!(help.contains("DISPLAY\n  display.list"));
    assert!(help.contains("WINDOW\n  window.capture"));
    assert!(help.contains("MEDIA CONTROL\n  mediaControl.nowPlaying"));
    assert!(!help.contains("debug."));
    assert!(!help.contains("verify."));
    assert!(!help.contains("music."));
  }

  #[test]
  fn command_help_describes_arguments_and_verification() {
    let registry = default_registry();
    let command = registry
      .resolve("window.capture")
      .expect("window.capture should exist");

    let help = render_command_help(command);

    assert!(help.contains("COMMAND\n  window.capture"));
    assert!(help.contains("DRIVER\n  macos.desktop.capture_window"));
    assert!(help.contains("--target"));
    assert!(help.contains("ARTIFACTS"));
    assert!(help.contains("VERIFY"));
  }

  #[test]
  fn registry_rejects_old_ids() {
    let registry = default_registry();

    assert!(registry.resolve("debug.captureWindow").is_none());
    assert!(registry.resolve("verify.axText").is_none());
    assert!(registry.resolve("music.result.play").is_none());
  }
}
```

- [ ] **Step 2: Run the focused tests and confirm they fail**

Run:

```bash
cargo test invoke::tests
```

Expected: fail to compile because `default_registry`, `render_help_index`, and `render_command_help` are not implemented.

- [ ] **Step 3: Implement the minimal metadata model and renderer**

Above the test module in `src/invoke.rs`, add:

```rust
use crate::model::{CommandSpec, DisturbanceClass};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InvokeNamespace {
  Display,
  Screen,
  Window,
  Input,
  App,
  Overlay,
  MediaControl,
  Fixture,
}

impl InvokeNamespace {
  fn heading(self) -> &'static str {
    match self {
      Self::Display => "DISPLAY",
      Self::Screen => "SCREEN",
      Self::Window => "WINDOW",
      Self::Input => "INPUT",
      Self::App => "APP",
      Self::Overlay => "OVERLAY",
      Self::MediaControl => "MEDIA CONTROL",
      Self::Fixture => "FIXTURE",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeArgSpec {
  pub flag: &'static str,
  pub value_name: &'static str,
  pub required: bool,
  pub help: &'static str,
}

#[derive(Clone, Debug)]
pub struct InvokeCommandSpec {
  pub command: CommandSpec,
  pub namespace: InvokeNamespace,
  pub args: &'static [InvokeArgSpec],
  pub artifacts: &'static [&'static str],
  pub signals: &'static [&'static str],
  pub verification: &'static str,
}

pub struct InvokeRegistry {
  commands: Vec<InvokeCommandSpec>,
}

impl InvokeRegistry {
  pub fn new(commands: Vec<InvokeCommandSpec>) -> Self {
    Self { commands }
  }

  pub fn resolve(&self, command_id: &str) -> Option<&InvokeCommandSpec> {
    self
      .commands
      .iter()
      .find(|command| command.command.id == command_id)
  }

  pub fn all(&self) -> &[InvokeCommandSpec] {
    &self.commands
  }
}

const NONE: &[DisturbanceClass] = &[DisturbanceClass::None];
const NONE_OR_FOREGROUND: &[DisturbanceClass] =
  &[DisturbanceClass::None, DisturbanceClass::ForegroundApp];

const TARGET_ARG: InvokeArgSpec = InvokeArgSpec {
  flag: "--target",
  value_name: "application-id",
  required: false,
  help: "Target application id or bundle id.",
};

const WINDOW_TITLE_ARG: InvokeArgSpec = InvokeArgSpec {
  flag: "--window-title",
  value_name: "text",
  required: false,
  help: "Window title substring used by the existing macOS selector.",
};

const LABEL_ARG: InvokeArgSpec = InvokeArgSpec {
  flag: "--label",
  value_name: "text",
  required: false,
  help: "Text label or query consumed by commands that resolve visible text.",
};

const TARGET_ARGS: &[InvokeArgSpec] = &[TARGET_ARG];
const WINDOW_ARGS: &[InvokeArgSpec] = &[TARGET_ARG, WINDOW_TITLE_ARG];
const LABEL_ARGS: &[InvokeArgSpec] = &[TARGET_ARG, LABEL_ARG];
const NO_ARGS: &[InvokeArgSpec] = &[];

pub fn render_help_index(registry: &InvokeRegistry) -> String {
  let mut output = String::from(
    "USAGE\n  auv-cli invoke <command> [options]\n\nUse `auv-cli invoke <command> --help` for command-specific options.\n",
  );
  let namespaces = [
    InvokeNamespace::Display,
    InvokeNamespace::Screen,
    InvokeNamespace::Window,
    InvokeNamespace::Input,
    InvokeNamespace::App,
    InvokeNamespace::Overlay,
    InvokeNamespace::MediaControl,
    InvokeNamespace::Fixture,
  ];
  for namespace in namespaces {
    let commands = registry
      .all()
      .iter()
      .filter(|command| command.namespace == namespace)
      .collect::<Vec<_>>();
    if commands.is_empty() {
      continue;
    }
    output.push_str("\n");
    output.push_str(namespace.heading());
    output.push('\n');
    for command in commands {
      output.push_str("  ");
      output.push_str(command.command.id);
      output.push_str("  ");
      output.push_str(command.command.summary);
      output.push('\n');
    }
  }
  output
}

pub fn render_command_help(command: &InvokeCommandSpec) -> String {
  let mut output = format!(
    "COMMAND\n  {}\n\nSUMMARY\n  {}\n\nDRIVER\n  {}.{}\n\nUSAGE\n  auv-cli invoke {}",
    command.command.id,
    command.command.summary,
    command.command.driver_id,
    command.command.operation,
    command.command.id,
  );
  for arg in command.args {
    output.push(' ');
    output.push_str(arg.flag);
    output.push_str(" <");
    output.push_str(arg.value_name);
    output.push('>');
  }
  output.push_str("\n\nDISTURBANCE\n  ");
  output.push_str(
    &command
      .command
      .disturbance_classes
      .iter()
      .map(|class| class.as_str())
      .collect::<Vec<_>>()
      .join(", "),
  );
  output.push_str("\n  max: ");
  output.push_str(command.command.max_disturbance.as_str());
  output.push_str("\n\nARGUMENTS\n");
  if command.args.is_empty() {
    output.push_str("  none\n");
  } else {
    for arg in command.args {
      output.push_str("  ");
      output.push_str(arg.flag);
      output.push_str(" <");
      output.push_str(arg.value_name);
      output.push_str(">  ");
      output.push_str(arg.help);
      if arg.required {
        output.push_str(" Required.");
      }
      output.push('\n');
    }
  }
  output.push_str("\nARTIFACTS\n");
  output.push_str(&render_list(command.artifacts));
  output.push_str("\nSIGNALS\n");
  output.push_str(&render_list(command.signals));
  output.push_str("\nVERIFY\n  ");
  output.push_str(command.verification);
  output.push('\n');
  output
}

fn render_list(values: &[&str]) -> String {
  if values.is_empty() {
    return "  none\n".to_string();
  }
  let mut output = String::new();
  for value in values {
    output.push_str("  ");
    output.push_str(value);
    output.push('\n');
  }
  output
}
```

- [ ] **Step 4: Add a small default registry so the tests can pass**

Still in `src/invoke.rs`, add:

```rust
pub fn default_registry() -> InvokeRegistry {
  InvokeRegistry::new(vec![
    spec(
      "display.list",
      InvokeNamespace::Display,
      "List connected displays using the normalized AUV coordinate contract.",
      "macos.desktop",
      "list_displays",
      NONE,
      DisturbanceClass::None,
      NO_ARGS,
      &[],
      &["display.count"],
      "read-only; no semantic success claim",
    ),
    spec(
      "window.capture",
      InvokeNamespace::Window,
      "Capture one single-display window and emit a coordinate contract.",
      "macos.desktop",
      "capture_window",
      NONE_OR_FOREGROUND,
      DisturbanceClass::ForegroundApp,
      WINDOW_ARGS,
      &["window-capture", "capture-contract"],
      &[],
      "capture-only; no semantic success claim",
    ),
    spec(
      "mediaControl.nowPlaying",
      InvokeNamespace::MediaControl,
      "Read the current system media session now-playing state.",
      "macos.desktop",
      "media_control_now_playing",
      NONE,
      DisturbanceClass::None,
      NO_ARGS,
      &["now-playing-v0"],
      &["mediaControl.present", "mediaControl.title", "mediaControl.isPlaying"],
      "read-only; success means the backend returned a structured state",
    ),
  ])
}

fn spec(
  id: &'static str,
  namespace: InvokeNamespace,
  summary: &'static str,
  driver_id: &'static str,
  operation: &'static str,
  disturbance_classes: &'static [DisturbanceClass],
  max_disturbance: DisturbanceClass,
  args: &'static [InvokeArgSpec],
  artifacts: &'static [&'static str],
  signals: &'static [&'static str],
  verification: &'static str,
) -> InvokeCommandSpec {
  InvokeCommandSpec {
    command: CommandSpec {
      id,
      summary,
      driver_id,
      operation,
      disturbance_classes,
      max_disturbance,
      namespace: crate::model::CommandNamespace::Observe,
    },
    namespace,
    args,
    artifacts,
    signals,
    verification,
  }
}
```

- [ ] **Step 5: Export the module**

Modify `src/lib.rs`:

```rust
pub mod invoke;
```

Keep `pub mod catalog;` for this task only; it is deleted later.

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test invoke::tests
```

Expected: all `invoke::tests` pass.

- [ ] **Step 7: Commit**

```bash
git add src/invoke.rs src/lib.rs
git commit -m "feat(invoke): add command metadata help renderer"
```

---

### Task 2: Build The Full Invoke Registry Alongside `catalog.rs`

**Files:**
- Modify: `src/invoke.rs`

- [ ] **Step 1: Add registry coverage tests**

In `src/invoke.rs` tests, add:

```rust
#[test]
fn default_registry_contains_expected_capability_ids() {
  let registry = default_registry();

  for id in [
    "display.capture",
    "display.list",
    "display.projectScreenshotPoint",
    "display.identifyPoint",
    "screen.findText",
    "screen.waitForText",
    "screen.clickText",
    "window.list",
    "window.capture",
    "window.captureAxTree",
    "window.verifyText",
    "input.key",
    "input.typeText",
    "input.pasteText",
    "input.clickPoint",
    "input.clickWindowPoint",
    "input.scrollPoint",
    "app.activate",
    "app.probePermissions",
    "overlay.showCursor",
    "overlay.shutdown",
    "fixture.observe",
  ] {
    assert!(registry.resolve(id).is_some(), "{id} should be registered");
  }
}

#[test]
fn default_registry_excludes_legacy_and_app_workflow_ids() {
  let registry = default_registry();

  for id in [
    "debug.captureDisplay",
    "debug.listWindows",
    "verify.axText",
    "verify.musicNowPlaying",
    "music.validate.candidate.liveness",
    "music.search.results",
    "music.result.play",
    "recognition.read.ratio",
    "steam.library.list.v0",
  ] {
    assert!(registry.resolve(id).is_none(), "{id} must not resolve");
  }
}

#[test]
fn default_registry_has_no_legacy_id_prefixes() {
  let registry = default_registry();

  for command in registry.all() {
    assert!(!command.command.id.starts_with("debug."), "{}", command.command.id);
    assert!(!command.command.id.starts_with("verify."), "{}", command.command.id);
    assert!(!command.command.id.starts_with("music."), "{}", command.command.id);
  }
}
```

- [ ] **Step 2: Run tests and confirm missing entries fail**

Run:

```bash
cargo test invoke::tests
```

Expected: `default_registry_contains_expected_capability_ids` fails until the full registry is moved.

- [ ] **Step 3: Expand constants in `src/invoke.rs`**

Add the remaining disturbance constants copied from `src/catalog.rs`:

```rust
const FOREGROUND_KEYBOARD: &[DisturbanceClass] =
  &[DisturbanceClass::ForegroundApp, DisturbanceClass::Keyboard];
const FOREGROUND_KEYBOARD_CLIPBOARD: &[DisturbanceClass] = &[
  DisturbanceClass::ForegroundApp,
  DisturbanceClass::Keyboard,
  DisturbanceClass::Clipboard,
];
const FOREGROUND_ONLY: &[DisturbanceClass] = &[DisturbanceClass::ForegroundApp];
const FOCUS_POINTER_ENTRY: &[DisturbanceClass] = &[
  DisturbanceClass::Focus,
  DisturbanceClass::ForegroundApp,
  DisturbanceClass::Keyboard,
  DisturbanceClass::Pointer,
];
const POINTER_WITH_FOREGROUND: &[DisturbanceClass] =
  &[DisturbanceClass::ForegroundApp, DisturbanceClass::Pointer];
const PRESS_BUTTON_DISTURBANCE: &[DisturbanceClass] = &[
  DisturbanceClass::ForegroundApp,
  DisturbanceClass::Keyboard,
  DisturbanceClass::Pointer,
];
const CAPTURE_AX_TREE_DISTURBANCE: &[DisturbanceClass] =
  &[DisturbanceClass::ForegroundApp, DisturbanceClass::Keyboard];
```

- [ ] **Step 4: Replace `default_registry()` with the full command list**

Use the rename map above. For each old `CommandSpec` in `src/catalog.rs`, create one `spec(...)` entry with the new id, same summary, same `driver_id`, same `operation`, same disturbance classes, and same `max_disturbance`. Exclude all app/domain-specific ids listed in the removal list.

For `window.verifyText`, reuse the old `verify.axText` operation and summary, but set `InvokeNamespace::Window` and verification text to:

```rust
"AX/window text verification; success requires a typed VerificationResult match"
```

Do not add a `mediaControl.*` entry in this task beyond `mediaControl.nowPlaying`; the full media commands are added after the driver adapter exists.

Keep `src/catalog.rs` and `pub mod catalog;` in place in this task. Runtime,
app probe, and scroll-scan tests still compile through the old catalog until
Task 3 moves those callers to the invoke registry. Deleting `src/catalog.rs`
before Task 3 would break the crate between commits.

- [ ] **Step 5: Run invoke tests**

Run:

```bash
cargo test invoke::tests
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add src/invoke.rs docs/ai/references/2026-06-11-auv-cli-invoke-driver-console-implementation-plan.md
git commit -m "refactor(invoke): build full command registry"
```

---

### Task 3: Make Runtime Execute Resolved Invoke Commands

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Modify tests importing `crate::catalog::CommandCatalog`
- Delete: `src/catalog.rs`

- [ ] **Step 1: Write runtime tests for resolved execution and unknown command ownership**

In `src/runtime.rs` tests, replace the unknown command test that expects `list-commands` with:

```rust
#[test]
fn invoke_unknown_command_points_to_invoke_help() {
  let runtime = test_runtime_with_commands(Vec::new());
  let error = runtime
    .invoke(crate::model::InvokeRequest {
      command_id: "missing.command".to_string(),
      ..Default::default()
    })
    .unwrap_err();

  assert!(error.contains("unknown command missing.command"));
  assert!(error.contains("auv-cli invoke --help"));
}
```

Add a helper test for direct resolved execution:

```rust
#[test]
fn invoke_resolved_uses_supplied_command_spec() {
  let runtime = test_runtime_with_commands(Vec::new());
  let command = crate::invoke::default_registry()
    .resolve("fixture.observe")
    .expect("fixture.observe should exist");

  let result = runtime
    .invoke_resolved(
      crate::model::InvokeRequest {
        command_id: "fixture.observe".to_string(),
        ..Default::default()
      },
      &command.command,
    )
    .expect("fixture command should execute");

  assert_eq!(result.status, crate::model::RunStatus::Completed);
}
```

If the existing test helper requires a `CommandCatalog`, first change it to build a runtime without commands in Step 3.

- [ ] **Step 2: Run the focused tests and confirm they fail**

Run:

```bash
cargo test runtime::tests
```

Expected: fail to compile because `Runtime::invoke_resolved` and commandless runtime construction do not exist yet.

- [ ] **Step 3: Remove command storage from `Runtime`**

In `src/runtime.rs`:

- Delete `use crate::catalog::CommandCatalog;`.
- Delete the `commands: CommandCatalog` field.
- Change constructors to:

```rust
pub fn new(project_root: PathBuf, drivers: DriverRegistry, store: LocalStore) -> Self {
  Self {
    project_root,
    drivers,
    recording: RunRecordingBackend::new(store, Arc::new(MemoryRunRecorder::new())),
  }
}
```

- Delete `new_with_catalogs`.
- Delete `list_commands`.
- Change `invoke_in_span` to resolve through `crate::invoke::default_registry()`:

```rust
pub fn invoke_in_span(
  &self,
  run: &mut crate::run_builder::RecordingRun,
  parent: &crate::run_builder::SpanRef,
  request: InvokeRequest,
) -> AuvResult<InvokeResult> {
  let command_id = request.command_id.clone();
  let registry = crate::invoke::default_registry();
  let command = registry.resolve(&command_id).ok_or_else(|| {
    format!("unknown command {command_id}; use `auv-cli invoke --help` to inspect available entries")
  })?;
  self.invoke_direct_command_in_span(run, parent, request, &command.command)
}
```

- Add:

```rust
pub fn invoke_resolved(
  &self,
  request: InvokeRequest,
  command: &CommandSpec,
) -> AuvResult<InvokeResult> {
  let mut run = self.start_run(crate::run_builder::RunSpec::new(
    RunType::Command,
    "auv.command",
  ))?;
  let root = run.root_span();
  let result = match self.invoke_direct_command_in_span(&mut run, &root, request, command) {
    Ok(result) => result,
    Err(error) => {
      if let Err(finish_error) = self.finish_run(
        run,
        crate::run_builder::RunFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(format!(
            "Invocation failed. Inspect the run for details: {error}"
          )),
          failure: Some(error.clone()),
        },
      ) {
        return Err(format!(
          "{error}; additionally failed to persist failed run: {finish_error}"
        ));
      }
      return Err(error);
    }
  };
  let status_code = if result.status == RunStatus::Completed {
    TraceStatusCode::Ok
  } else {
    TraceStatusCode::Error
  };
  self.finish_run(
    run,
    crate::run_builder::RunFinish {
      status_code,
      summary: Some(result.output_summary.clone()),
      failure: result.failure_message.clone(),
    },
  )?;
  Ok(result)
}
```

Keep `invoke_direct_command_in_span` private.

- [ ] **Step 4: Update `src/lib.rs` runtime construction**

Replace:

```rust
let commands = default_command_catalog();
let drivers = default_driver_registry();
Ok(Runtime::new_with_catalogs(
  project_root,
  commands,
  drivers,
  store,
))
```

with:

```rust
let drivers = default_driver_registry();
Ok(Runtime::new(project_root, drivers, store))
```

Delete:

```rust
use catalog::default_command_catalog;
```

Also delete:

```rust
pub mod catalog;
```

- [ ] **Step 5: Update tests that constructed `CommandCatalog`**

For `src/runtime.rs`, replace helper construction like:

```rust
Runtime::new_with_catalogs(project_root, CommandCatalog::new(Vec::new()), drivers, store)
```

with:

```rust
Runtime::new(project_root, drivers, store)
```

For `src/scroll_scan/mod.rs` tests that need invokeable commands in spans, replace explicit `CommandCatalog::new(...)` setup with `Runtime::new(...)`; runtime now resolves from `invoke::default_registry()`. Update hardcoded command ids:

- `debug.observeWindowRegion` -> `window.observeRegion`
- `debug.scrollWindowRegion` -> `window.scrollRegion`
- `debug.fixtureObserve` -> `fixture.observe`

For `src/app/tests.rs`, remove `use crate::catalog::CommandCatalog;` and construct `Runtime::new(...)` with the existing test driver registry.

- [ ] **Step 6: Keep `list-commands` compiling through invoke registry until Task 4**

In `src/main.rs`, update the existing `CliCommand::ListCommands` arm to avoid
`runtime.list_commands()`. This is a temporary compile-preserving bridge until
Task 4 removes `list-commands` from the CLI parser:

```rust
CliCommand::ListCommands => {
  let registry = auv_cli::invoke::default_registry();
  for command in registry.all() {
    println!(
      "{} -> {}.{}",
      command.command.id, command.command.driver_id, command.command.operation
    );
    println!("  {}", command.command.summary);
    println!(
      "  disturbance: {} (max: {})",
      command
        .command
        .disturbance_classes
        .iter()
        .map(|class| class.as_str())
        .collect::<Vec<_>>()
        .join(", "),
      command.command.max_disturbance.as_str()
    );
  }
}
```

- [ ] **Step 7: Delete `src/catalog.rs` after callers are migrated**

Run:

```bash
rg "crate::catalog::CommandCatalog|default_command_catalog|mod catalog|pub mod catalog" src -g '*.rs'
```

Expected: no remaining production references after the edits above.

Then run:

```bash
git rm src/catalog.rs
```

- [ ] **Step 8: Run affected tests**

Run:

```bash
cargo test runtime::tests
cargo test scroll_scan::tests
cargo test app::tests
cargo test cli::tests
```

Expected: pass.

- [ ] **Step 9: Commit**

```bash
git add src/runtime.rs src/lib.rs src/main.rs src/scroll_scan/mod.rs src/app/tests.rs src/catalog.rs docs/ai/references/2026-06-11-auv-cli-invoke-driver-console-implementation-plan.md
git commit -m "refactor(runtime): execute resolved invoke commands"
```

---

### Task 4: Replace CLI Discovery With `invoke --help`

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add parser tests**

In `src/cli.rs` tests, add:

```rust
#[test]
fn parse_invoke_help() {
  let command = parse_cli(&["invoke".to_string(), "--help".to_string()]).unwrap();

  assert!(matches!(command, CliCommand::InvokeHelp));
}

#[test]
fn parse_invoke_command_help() {
  let command = parse_cli(&[
    "invoke".to_string(),
    "window.capture".to_string(),
    "--help".to_string(),
  ])
  .unwrap();

  assert!(matches!(
    command,
    CliCommand::InvokeCommandHelp { command_id } if command_id == "window.capture"
  ));
}

#[test]
fn parse_list_commands_is_removed() {
  let error = parse_cli(&["list-commands".to_string()]).unwrap_err();

  assert!(error.contains("list-commands has been removed"));
  assert!(error.contains("auv-cli invoke --help"));
}

#[test]
fn parse_invoke_accepts_new_command_id() {
  let command = parse_cli(&[
    "invoke".to_string(),
    "window.capture".to_string(),
    "--target".to_string(),
    "com.apple.TextEdit".to_string(),
  ])
  .unwrap();

  match command {
    CliCommand::Invoke { request, .. } => {
      assert_eq!(request.command_id, "window.capture");
      assert_eq!(request.target.application_id.as_deref(), Some("com.apple.TextEdit"));
    }
    other => panic!("unexpected command: {other:?}"),
  }
}
```

- [ ] **Step 2: Run parser tests and confirm failures**

Run:

```bash
cargo test cli::tests
```

Expected: fail to compile until new `CliCommand` variants exist and old `list-commands` parser is removed.

- [ ] **Step 3: Update `CliCommand` and top-level parse**

In `src/cli.rs`, replace:

```rust
ListCommands,
```

with:

```rust
InvokeHelp,
InvokeCommandHelp {
  command_id: String,
},
```

Change the top-level match arm:

```rust
"list-commands" => Err(
  "list-commands has been removed; use `auv-cli invoke --help` to inspect available commands"
    .to_string(),
),
```

- [ ] **Step 4: Update `parse_invoke`**

At the top of `parse_invoke`, use:

```rust
if arguments.len() == 2 && matches!(arguments[1].as_str(), "--help" | "-h") {
  return Ok(CliCommand::InvokeHelp);
}

if arguments.len() == 3 && matches!(arguments[2].as_str(), "--help" | "-h") {
  return Ok(CliCommand::InvokeCommandHelp {
    command_id: arguments[1].clone(),
  });
}
```

Keep generic flag parsing for execution.

- [ ] **Step 5: Rewrite `help_text()`**

Remove `auv-cli list-commands` from usage. Replace all old debug/verify/music notes with:

```text
  auv-cli invoke --help
  auv-cli invoke <command> --help
  auv-cli invoke <command> [--dry-run] [--target <application-id>] [--label <text>] [inspect options...]
```

Add this note:

```text
  - `invoke --help` is the command index for driver capability commands.
  - Command ids use capability namespaces such as display.*, screen.*, window.*, input.*, app.*, overlay.*, and mediaControl.*.
  - App-specific workflows live in app-local CLIs, not generic invoke commands.
```

- [ ] **Step 6: Update `src/main.rs` match arms**

Delete the `CliCommand::ListCommands` arm.

Add:

```rust
CliCommand::InvokeHelp => {
  let registry = auv_cli::invoke::default_registry();
  print!("{}", auv_cli::invoke::render_help_index(&registry));
}
CliCommand::InvokeCommandHelp { command_id } => {
  let registry = auv_cli::invoke::default_registry();
  let command = registry.resolve(&command_id).ok_or_else(|| {
    format!("unknown command {command_id}; use `auv-cli invoke --help` to inspect available entries")
  })?;
  print!("{}", auv_cli::invoke::render_command_help(command));
}
```

In the existing `CliCommand::Invoke` arm, resolve before runtime execution:

```rust
let registry = auv_cli::invoke::default_registry();
let command = registry.resolve(&request.command_id).ok_or_else(|| {
  format!("unknown command {}; use `auv-cli invoke --help` to inspect available entries", request.command_id)
})?;
let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
let result = runtime.invoke_resolved(request, &command.command)?;
```

- [ ] **Step 7: Run CLI tests**

Run:

```bash
cargo test cli::tests
```

Expected: pass after updating old tests that still assert `debug.captureDisplay` to use `display.capture`.

- [ ] **Step 8: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat(cli): make invoke help the command index"
```

---

### Task 5: Add Generic `mediaControl.*` Driver Operations

**Files:**
- Modify: `Cargo.toml`
- Create: `src/driver/macos/media_control.rs`
- Modify: `src/driver/macos/mod.rs`
- Modify: `src/driver/macos/dispatch.rs`
- Modify: `src/invoke.rs`

- [ ] **Step 1: Add unit tests for media verification helpers**

Create `src/driver/macos/media_control.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  fn state(title: &str, is_playing: bool, elapsed_seconds: f64) -> auv_media_macos::NowPlayingState {
    auv_media_macos::NowPlayingState {
      present: true,
      title: Some(title.to_string()),
      is_playing,
      elapsed_seconds: Some(elapsed_seconds),
      ..Default::default()
    }
  }

  #[test]
  fn playback_verification_reports_verified_state() {
    assert_eq!(
      verify_playback_state(Some(false), &state("Song", false, 10.0)),
      "verified"
    );
  }

  #[test]
  fn playback_verification_reports_inconclusive_when_state_is_not_target() {
    assert_eq!(
      verify_playback_state(Some(false), &state("Song", true, 10.0)),
      "inconclusive"
    );
  }

  #[test]
  fn track_transition_verification_accepts_identity_change() {
    let before = state("First", true, 40.0);
    let after = state("Second", true, 2.0);

    assert_eq!(verify_track_transition(&before, &after), "verified");
  }

  #[test]
  fn track_transition_verification_reports_inconclusive_without_change() {
    let before = state("First", true, 40.0);
    let after = state("First", true, 40.5);

    assert_eq!(verify_track_transition(&before, &after), "inconclusive");
  }
}
```

- [ ] **Step 2: Add root dependency**

In root `Cargo.toml`, under `[target.'cfg(target_os = "macos")'.dependencies]`, add:

```toml
auv-media-macos = { path = "crates/auv-media-macos" }
```

- [ ] **Step 3: Implement `media_control.rs`**

Use this structure:

```rust
use auv_media_macos::{MediaCommand, NowPlayingState};

use super::support::artifacts::build_text_artifact;
use crate::model::{AuvResult, DriverCall, DriverResponse};

pub(crate) fn media_control_now_playing(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let state = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  response_for_state("mediaControl.nowPlaying", state, "verified")
}

pub(crate) fn media_control_play(_call: &DriverCall) -> AuvResult<DriverResponse> {
  send_and_verify_playback(MediaCommand::Play, Some(true))
}

pub(crate) fn media_control_pause(_call: &DriverCall) -> AuvResult<DriverResponse> {
  send_and_verify_playback(MediaCommand::Pause, Some(false))
}

pub(crate) fn media_control_toggle_play_pause(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let before = auv_media_macos::now_playing().ok();
  auv_media_macos::send_command(MediaCommand::TogglePlayPause).map_err(|error| error.to_string())?;
  let after = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  let verification = before
    .as_ref()
    .map(|state| if state.is_playing != after.is_playing { "verified" } else { "inconclusive" })
    .unwrap_or("inconclusive");
  response_for_state("mediaControl.togglePlayPause", after, verification)
}

pub(crate) fn media_control_next(_call: &DriverCall) -> AuvResult<DriverResponse> {
  send_and_verify_track_transition(MediaCommand::NextTrack)
}

pub(crate) fn media_control_previous(_call: &DriverCall) -> AuvResult<DriverResponse> {
  send_and_verify_track_transition(MediaCommand::PreviousTrack)
}

fn send_and_verify_playback(
  command: MediaCommand,
  expected_is_playing: Option<bool>,
) -> AuvResult<DriverResponse> {
  auv_media_macos::send_command(command).map_err(|error| error.to_string())?;
  let after = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  let verification = verify_playback_state(expected_is_playing, &after);
  response_for_state(
    &format!("mediaControl.{}", command.label()),
    after,
    verification,
  )
}

fn send_and_verify_track_transition(command: MediaCommand) -> AuvResult<DriverResponse> {
  let before = auv_media_macos::now_playing().ok();
  auv_media_macos::send_command(command).map_err(|error| error.to_string())?;
  let after = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  let verification = before
    .as_ref()
    .map(|before| verify_track_transition(before, &after))
    .unwrap_or("inconclusive");
  response_for_state(
    &format!("mediaControl.{}", command.label()),
    after,
    verification,
  )
}

fn verify_playback_state(expected: Option<bool>, after: &NowPlayingState) -> &'static str {
  match expected {
    Some(expected) if after.present && after.is_playing == expected => "verified",
    Some(_) => "inconclusive",
    None => "inconclusive",
  }
}

fn verify_track_transition(before: &NowPlayingState, after: &NowPlayingState) -> &'static str {
  if !after.present {
    return "inconclusive";
  }
  if before.content_item_id.is_some()
    && after.content_item_id.is_some()
    && before.content_item_id != after.content_item_id
  {
    return "verified";
  }
  if before.title.is_some() && after.title.is_some() && before.title != after.title {
    return "verified";
  }
  if before.elapsed_seconds.unwrap_or(0.0) > 5.0 && after.elapsed_seconds.unwrap_or(0.0) < 3.0 {
    return "verified";
  }
  "inconclusive"
}

fn response_for_state(
  operation_id: &str,
  state: NowPlayingState,
  verification: &str,
) -> AuvResult<DriverResponse> {
  let json = serde_json::to_string_pretty(&auv_media_macos::output::build_now_playing_output(&state))
    .unwrap_or_else(|error| format!(r#"{{"error":"failed to encode now-playing: {error}"}}"#));
  let artifact = build_text_artifact(
    "now-playing-v0",
    "json",
    "now-playing",
    json,
    "System media now-playing state from auv-media-macos.",
  )?;
  let mut signals = std::collections::BTreeMap::new();
  signals.insert("mediaControl.present".to_string(), state.present.to_string());
  signals.insert("mediaControl.isPlaying".to_string(), state.is_playing.to_string());
  signals.insert("mediaControl.verification".to_string(), verification.to_string());
  if let Some(title) = state.title.as_deref() {
    signals.insert("mediaControl.title".to_string(), title.to_string());
  }
  if let Some(bundle_id) = state.source_bundle_id.as_deref() {
    signals.insert("mediaControl.sourceBundleId".to_string(), bundle_id.to_string());
  }
  Ok(DriverResponse {
    summary: format!("{operation_id}: verification={verification}"),
    backend: Some("auv-media-macos".to_string()),
    signals,
    notes: Vec::new(),
    artifacts: vec![artifact],
  })
}
```

- [ ] **Step 4: Wire dispatch**

In `src/driver/macos/mod.rs` add:

```rust
mod media_control;
```

In `src/driver/macos/dispatch.rs`, import:

```rust
use super::media_control::{
  media_control_next, media_control_now_playing, media_control_pause, media_control_play,
  media_control_previous, media_control_toggle_play_pause,
};
```

Add a dispatch group before overlay:

```rust
if let Some(response) = dispatch_media_control_operation(call) {
  return response;
}
```

Add:

```rust
fn dispatch_media_control_operation(call: &DriverCall) -> Option<AuvResult<DriverResponse>> {
  Some(match call.operation.as_str() {
    "media_control_now_playing" => media_control_now_playing(call),
    "media_control_play" => media_control_play(call),
    "media_control_pause" => media_control_pause(call),
    "media_control_toggle_play_pause" => media_control_toggle_play_pause(call),
    "media_control_next" => media_control_next(call),
    "media_control_previous" => media_control_previous(call),
    _ => return None,
  })
}
```

- [ ] **Step 5: Register all `mediaControl.*` ids**

In `src/invoke.rs`, add specs for:

- `mediaControl.play`
- `mediaControl.pause`
- `mediaControl.togglePlayPause`
- `mediaControl.next`
- `mediaControl.previous`

Each uses driver `macos.desktop`, the operation from the media table, `NONE`, `DisturbanceClass::None`, `NO_ARGS`, artifact `now-playing-v0`, signal `mediaControl.verification`, and verification text that names verified/inconclusive behavior.

- [ ] **Step 6: Run media tests**

Run:

```bash
cargo test driver::macos::media_control::tests
cargo test invoke::tests
```

Expected: pass on macOS and compile-gated root target.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/driver/macos/media_control.rs src/driver/macos/mod.rs src/driver/macos/dispatch.rs src/invoke.rs
git commit -m "feat(invoke): add generic media control commands"
```

---

### Task 6: Remove Legacy Operation Exposure From Generic Invoke

**Files:**
- Modify: `src/driver/macos/dispatch.rs`
- Modify: `src/driver/macos/control/mod.rs`
- Modify tests under `src/driver/macos/control/music.rs` only if imports become unused

- [ ] **Step 1: Add a dispatch negative test**

In `src/driver/macos/dispatch.rs`, add a `#[cfg(test)]` module with:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  fn call(operation: &str) -> DriverCall {
    DriverCall {
      operation: operation.to_string(),
      target: Default::default(),
      inputs: Default::default(),
      working_directory: std::path::PathBuf::from("."),
      run_context: Default::default(),
    }
  }

  #[test]
  fn dispatch_does_not_expose_app_workflow_operations() {
    for operation in [
      "music_search_results",
      "music_result_play",
      "music_validate_candidate_liveness",
      "recognition_read_ratio",
    ] {
      assert!(dispatch_observe_operation(&call(operation)).is_none(), "{operation}");
      assert!(dispatch_control_operation(&call(operation)).is_none(), "{operation}");
    }
  }
}
```

If `DriverRunContext` has no `Default`, construct it with stable string ids:

```rust
run_context: DriverRunContext {
  run_id: "run_test".to_string(),
  span_id: "span_test".to_string(),
  device_id: "device_test".to_string(),
  session_id: "session_test".to_string(),
},
```

- [ ] **Step 2: Run test and confirm it fails**

Run:

```bash
cargo test driver::macos::dispatch::tests::dispatch_does_not_expose_app_workflow_operations
```

Expected: fail because the app workflow operations are still dispatched.

- [ ] **Step 3: Remove dispatch arms**

In `src/driver/macos/dispatch.rs`, delete imports and match arms for:

- `music_result_play`
- `music_search_results`
- `music_validate_candidate_liveness`
- `recognition_read_ratio`

Do not delete the implementation modules in this PR unless the compiler proves they are now unreachable and unused. The goal is removal from generic invoke, not broad source deletion.

- [ ] **Step 4: Run dispatch tests**

Run:

```bash
cargo test driver::macos::dispatch::tests::dispatch_does_not_expose_app_workflow_operations
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src/driver/macos/dispatch.rs src/driver/macos/control/mod.rs
git commit -m "refactor(invoke): stop dispatching app workflow operations"
```

---

### Task 7: Update Downstream References And Smoke Commands

**Files:**
- Modify: `src/app/mod.rs`
- Modify: `src/app/tests.rs`
- Modify: `src/scroll_scan/mod.rs`
- Modify: `src/model.rs`
- Modify: any compile errors from `rg "debug\\.|verify\\.|music\\." src -g '*.rs'`

- [ ] **Step 1: Update command ids that intentionally call generic invoke**

Use these replacements:

- `debug.probePermissions` -> `app.probePermissions`
- `debug.listDisplays` -> `display.list`
- `debug.probeCoordinateReadiness` -> `display.probeCoordinateReadiness`
- `debug.activateApp` -> `app.activate`
- `debug.listWindows` -> `window.list`
- `debug.captureAxTree` -> `window.captureAxTree`
- `debug.captureWindow` -> `window.capture`
- `debug.observeWindowRegion` -> `window.observeRegion`
- `debug.scrollWindowRegion` -> `window.scrollRegion`
- `debug.fixtureObserve` -> `fixture.observe`

Leave historical artifact operation ids in tests or docs only when they are testing old stored run data; add a short comment that the string is historical fixture data.

- [ ] **Step 2: Update model comments**

In `src/model.rs`, replace comments that describe `debug.*` as the current command surface with comments that describe capability namespaces. Remove `music.result.play` as an example for generic invoke.

- [ ] **Step 3: Run targeted compile/test loop**

Run:

```bash
cargo test app::tests
cargo test scroll_scan::tests
cargo test runtime::tests
cargo test cli::tests
cargo test invoke::tests
```

Expected: pass.

- [ ] **Step 4: Run old-id search**

Run:

```bash
rg -n 'debug\\.|verify\\.|music\\.' src -g '*.rs'
```

Expected:

- No positive invoke registry or CLI help references to `debug.*`, `verify.*`, or `music.*`.
- Remaining hits are allowed only in historical fixture tests, archived comments, or app-local/non-invoke implementation code. Each allowed remaining hit must be either outside generic invoke or accompanied by a comment explaining why it is historical or app-local.

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs src/app/tests.rs src/scroll_scan/mod.rs src/model.rs
git commit -m "chore(invoke): update internal command ids"
```

---

### Task 8: Final Verification

**Files:**
- No planned source edits unless verification finds a defect.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: no errors.

- [ ] **Step 2: Workspace check**

Run:

```bash
cargo check
```

Expected: pass.

- [ ] **Step 3: Full tests**

Run:

```bash
cargo test
```

Expected: pass.

- [ ] **Step 4: Whitespace check**

Run:

```bash
git diff --check
```

Expected: no output.

- [ ] **Step 5: CLI help smoke**

Run:

```bash
cargo run --quiet -- invoke --help
```

Expected output contains:

```text
DISPLAY
  display.list
WINDOW
  window.capture
MEDIA CONTROL
  mediaControl.nowPlaying
```

Expected output does not contain:

```text
debug.
verify.
music.
```

- [ ] **Step 6: Command help smoke**

Run:

```bash
cargo run --quiet -- invoke window.capture --help
cargo run --quiet -- invoke mediaControl.nowPlaying --help
```

Expected: both print command-specific help with `DRIVER`, `ARGUMENTS`, `ARTIFACTS`, and `VERIFY`.

- [ ] **Step 7: Negative old-id smoke**

Run:

```bash
cargo run --quiet -- invoke debug.listDisplays
cargo run --quiet -- invoke verify.musicNowPlaying
cargo run --quiet -- invoke music.result.play
```

Expected: each exits non-zero and says to use `auv-cli invoke --help`. None executes a driver operation.

- [ ] **Step 8: Low-disturbance positive smoke**

Run:

```bash
cargo run --quiet -- invoke display.list
```

Expected: completes and records a run, printing `runId:`, `status: completed`, and display output/artifacts.

- [ ] **Step 9: Media help smoke**

Run:

```bash
cargo run --quiet -- invoke mediaControl.pause --help
```

Expected: help documents verified/inconclusive semantics.

- [ ] **Step 10: Final commit if formatting changed files**

If `cargo fmt` changed files after the previous task commits:

```bash
git add .
git commit -m "chore: format invoke driver console changes"
```

---

### Task 9: Extract Invoke Into Crates And Move Operation Metadata To Driver

**Reason for amendment:**
The first implementation removed `src/catalog.rs`, but kept the replacement
registry in root `src/invoke.rs`. That preserves too much catalog-like gravity
inside the root CLI crate. The intended boundary is:

- `auv-driver` owns operation metadata for atomic driver capabilities.
- `auv-cli-invoke` owns CLI/invoke command registration, argument metadata, and
  help rendering.
- Root `auv-cli` remains CLI/runtime/MCP glue and no longer owns a generic
  invoke registry module.

**Files:**
- Add: `crates/auv-cli-invoke/Cargo.toml`
- Add: `crates/auv-cli-invoke/src/lib.rs`
- Add: `crates/auv-cli-invoke/src/arg.rs`
- Add: `crates/auv-cli-invoke/src/command.rs`
- Add: `crates/auv-cli-invoke/src/help.rs`
- Add: `crates/auv-cli-invoke/src/registry.rs`
- Add: `crates/auv-cli-invoke/src/commands/*.rs`
- Add: `crates/auv-driver/src/operation.rs`
- Modify: `crates/auv-driver/src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `src/model.rs`
- Modify: `src/runtime.rs`
- Modify: `src/main.rs`
- Modify: `src/mcp.rs`
- Delete: `src/invoke.rs`

- [ ] **Step 1: Move operation metadata into `auv-driver`**

Create `auv_driver::operation` with:

- `OperationDisturbance`
- `OperationNamespace`
- `OperationSpec`

These replace root `DisturbanceClass`, `CommandNamespace`, and `CommandSpec`.
The names must describe driver operations, not catalog or invoke commands.

- [ ] **Step 2: Add `auv-cli-invoke` crate**

Create a workspace crate named `auv-cli-invoke`. It depends on `auv-driver`
and owns:

- `ArgSpec`
- `InvokeCommand`
- `InvokeNamespace`
- `InvokeRegistry`
- `default_registry`
- help rendering

Use `src/commands/*.rs` for grouped registrations:

- `display.rs`
- `screen.rs`
- `window.rs`
- `input.rs`
- `app.rs`
- `overlay.rs`
- `media_control.rs`
- `fixture.rs`

Do not use a generic `domains` module name.

- [ ] **Step 3: Rewire root consumers**

Update root `auv-cli`:

- `main.rs` imports `auv_cli_invoke::{default_registry, render_command_help, render_help_index}`.
- `mcp.rs` imports `auv_cli_invoke::{ArgSpec, InvokeCommand, default_registry}`.
- `runtime.rs` resolves through `auv_cli_invoke::default_registry()` only at
  the invoke boundary, then executes the command's `OperationSpec`.
- `model.rs` no longer defines operation metadata types.
- `lib.rs` no longer exports `pub mod invoke`.

- [ ] **Step 4: Preserve behavior**

Keep all behavior from Tasks 1-8:

- `invoke --help` and `invoke <command> --help` output remains stable.
- Unknown old ids still fail after recording a failed run.
- MCP schema still exposes registry-backed enum and `x-auv-commands`.
- No `debug.*`, `verify.*`, or `music.*` command ids return to the generic
  invoke registry.

- [ ] **Step 5: Tests and search**

Run:

```bash
cargo test -p auv-cli-invoke
cargo test runtime::tests
cargo test mcp::tests
cargo test cli::tests
rg -n 'crate::invoke|pub mod invoke|src/invoke.rs|debug\.|verify\.|music\.' src crates/auv-cli-invoke crates/auv-driver -g '*.rs'
```

Expected:

- No root `crate::invoke` dependency remains.
- Old id hits remain only in negative tests, historical fixtures, durable
  non-invoke operation-result ids, or app-local/test-only code with comments.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/auv-driver crates/auv-cli-invoke src docs/ai/references/2026-06-11-auv-cli-invoke-driver-console-implementation-plan.md
git commit -m "refactor(invoke): extract invoke registry crate"
```

---

## Self-Review Checklist

- Spec coverage:
  - `invoke --help` covered by Tasks 1 and 4.
  - `invoke <command> --help` covered by Tasks 1 and 4.
  - No legacy aliases covered by Tasks 2, 4, 6, and 8.
  - `src/catalog.rs` deletion covered by Task 2.
  - Runtime no longer owns command discovery covered by Task 3.
  - `mediaControl.*` covered by Task 5.
  - App-specific `music.*` removal covered by Tasks 2 and 6.
- Scope boundaries:
  - No REPL work.
  - No tracing-driver split.
  - No JSON recipe or bundle compatibility.
  - No broad deletion of old app/domain implementation modules beyond removing generic invoke dispatch exposure.
- Risk:
  - If `mediaControl.*` driver code cannot compile cleanly because `DriverResponse` fields differ, adapt only the struct construction while preserving artifact role `now-playing-v0`, `mediaControl.*` signals, and verified/inconclusive semantics.
  - If direct root module extraction creates too much churn, keep the module-level deferral marker and do not create a workspace crate in this PR.
