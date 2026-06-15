//! Registry-backed CLI invoke metadata and help rendering.
//!
//! `auv-driver` owns the operation contract. This crate owns how those
//! operations are exposed as explicit `auv-cli invoke ...` commands.

extern crate self as auv_cli_invoke;

pub mod arg;
pub mod command;
pub mod commands;
pub mod help;
pub mod registry;

pub use arg::ArgSpec;
pub use auv_cli_invoke_macros::invoke_command;
pub use command::{
  CommandGroup, CommandNode, InvokeCommand, InvokeCommandHandler, InvokeContext,
  InvokeDriverDispatch, InvokeNamespace, default_driver_dispatch,
};
pub use help::{render_command_help, render_help_index};
pub use registry::{InvokeRegistry, default_registry};

#[cfg(test)]
mod tests {
  use auv_driver::{OperationDisturbance, OperationNamespace};

  use super::{
    CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch, InvokeNamespace,
    InvokeRegistry, default_driver_dispatch, default_registry, invoke_command, render_command_help,
    render_help_index,
  };

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
  fn help_index_groups_commands_by_namespace() {
    let registry = default_registry();
    let help = render_help_index(&registry);

    assert!(help.contains("DISPLAY\n"));
    assert!(help.contains("  display.list"));
    assert!(help.contains("WINDOW\n"));
    assert!(help.contains("  window.capture"));
    assert!(help.contains("INPUT\n"));
    assert!(help.contains("  input.key"));
    assert!(help.contains("MEDIA CONTROL\n"));
    assert!(help.contains("  mediaControl.nowPlaying"));
    assert!(help.contains("STEAM\n"));
    assert!(help.contains("  steam.library.list.v0"));
    assert!(!help.contains("debug."));
    assert!(!help.contains("verify."));
    assert!(!help.contains("music."));
  }

  #[test]
  fn command_help_describes_arguments_and_verification() {
    let registry = default_registry();
    let command = registry
      .resolve("window.capture")
      .expect("window.capture should be registered");
    let help = render_command_help(command);

    assert!(help.contains("COMMAND\n  window.capture"));
    assert!(help.contains("DRIVER\n  macos.desktop.capture_window"));
    assert!(help.contains("--target APP  optional"));
    assert!(help.contains("ARTIFACTS\n  window-capture\n  capture-contract"));
    assert!(help.contains("VERIFY\n  capture-only; no semantic success claim"));
  }

  #[test]
  fn command_help_uses_driver_input_flags_for_concrete_commands() {
    let registry = default_registry();
    let command = registry
      .resolve("input.typeText")
      .expect("input.typeText should be registered");
    let help = render_command_help(command);

    assert!(help.contains("USAGE\n  auv-cli invoke input.typeText [--target APP] --text TEXT"));
    assert!(help.contains("--text TEXT  required"));
    assert!(help.contains("DISTURBANCE\n  max: keyboard"));
  }

  #[test]
  fn default_registry_sets_underlying_operation_namespaces() {
    let registry = default_registry();
    for (id, expected) in [
      ("input.key", OperationNamespace::Action),
      ("app.activate", OperationNamespace::Action),
      ("overlay.showCursor", OperationNamespace::Overlay),
      ("window.verifyText", OperationNamespace::Verify),
      ("display.list", OperationNamespace::Observe),
      ("mediaControl.nowPlaying", OperationNamespace::Observe),
      ("mediaControl.play", OperationNamespace::Action),
      ("mediaControl.pause", OperationNamespace::Action),
      ("mediaControl.togglePlayPause", OperationNamespace::Action),
      ("mediaControl.next", OperationNamespace::Action),
      ("mediaControl.previous", OperationNamespace::Action),
      ("steam.library.list.v0", OperationNamespace::Observe),
    ] {
      let command = registry.resolve(id).expect("command should be registered");
      assert_eq!(command.operation.namespace, expected, "{id}");
    }
  }

  #[test]
  fn registry_supports_nested_command_groups() {
    let fixture = default_registry()
      .resolve("fixture.observe")
      .expect("fixture command should exist")
      .clone();
    let registry = InvokeRegistry::from_groups(vec![
      CommandGroup::new("root", "ROOT").group(CommandGroup::new("child", "CHILD").command(fixture)),
    ]);

    assert!(registry.resolve("fixture.observe").is_some());
    let help = render_help_index(&registry);
    assert!(help.contains("ROOT\n"));
    assert!(help.contains("  CHILD\n"));
    assert!(help.contains("    fixture.observe"));
  }

  #[test]
  #[should_panic(expected = "duplicate invoke command id registered: fixture.observe")]
  fn registry_rejects_duplicate_command_ids() {
    let fixture = default_registry()
      .resolve("fixture.observe")
      .expect("fixture command should exist")
      .clone();

    let _registry = InvokeRegistry::from_groups(vec![
      CommandGroup::new("one", "ONE").command(fixture.clone()),
      CommandGroup::new("two", "TWO").command(fixture),
    ]);
  }

  #[test]
  fn command_handler_builds_default_driver_dispatch() {
    let registry = default_registry();
    let command = registry
      .resolve("fixture.observe")
      .expect("fixture.observe should be registered");
    let mut inputs = std::collections::BTreeMap::new();
    inputs.insert("scene".to_string(), "demo".to_string());
    let context = crate::InvokeContext {
      target_application_id: Some("com.example.App"),
      target_label: Some("Example"),
      inputs: &inputs,
    };

    let dispatch = command.dispatch(context);

    assert_eq!(dispatch.command_id, "fixture.observe");
    assert_eq!(dispatch.driver_id, "fixture.observe");
    assert_eq!(dispatch.operation, "observe_fixture_scene");
    assert_eq!(
      dispatch.target_application_id.as_deref(),
      Some("com.example.App")
    );
    assert_eq!(dispatch.target_label.as_deref(), Some("Example"));
    assert_eq!(
      dispatch.inputs.get("scene").map(String::as_str),
      Some("demo")
    );
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

  #[test]
  fn help_index_skips_empty_nested_groups() {
    let fixture = default_registry()
      .resolve("fixture.observe")
      .expect("fixture command should exist")
      .clone();
    let registry = InvokeRegistry::from_groups(vec![
      CommandGroup::new("root", "ROOT")
        .group(CommandGroup::new("empty", "EMPTY"))
        .group(CommandGroup::new("child", "CHILD").command(fixture)),
    ]);

    let help = render_help_index(&registry);
    assert!(help.contains("ROOT\n"));
    assert!(help.contains("  CHILD\n"));
    assert!(!help.contains("EMPTY\n"));
  }

  #[test]
  fn registry_rejects_old_ids() {
    let registry = default_registry();

    assert!(registry.resolve("debug.captureWindow").is_none());
    assert!(registry.resolve("verify.axText").is_none());
    assert!(registry.resolve("music.result.play").is_none());
  }

  #[test]
  fn default_registry_contains_expected_capability_ids() {
    let registry = default_registry();
    for id in [
      "display.capture",
      "display.list",
      "display.projectScreenshotPoint",
      "display.identifyPoint",
      "display.probeCoordinateReadiness",
      "screen.captureRegion",
      "screen.findText",
      "screen.waitForText",
      "screen.findRows",
      "screen.waitForRows",
      "screen.findImageText",
      "screen.clickText",
      "screen.clickRow",
      "window.list",
      "window.capture",
      "window.captureAxTree",
      "window.findText",
      "window.waitForText",
      "window.findRows",
      "window.waitForRows",
      "window.observeRegion",
      "window.findIconMatch",
      "window.scrollRegion",
      "window.verifyText",
      "window.clickText",
      "window.clickRow",
      "input.focusText",
      "input.pressButton",
      "input.axPressButton",
      "input.axFocusText",
      "input.axClickWindowText",
      "input.smartPress",
      "input.typeText",
      "input.pasteText",
      "input.key",
      "input.clickPoint",
      "input.clickWindowPoint",
      "input.teachClick",
      "input.scrollPoint",
      "app.probePermissions",
      "app.activate",
      "overlay.clickPoint",
      "overlay.showCursor",
      "overlay.showDualCursor",
      "overlay.applyCursorBatch",
      "overlay.setCursor",
      "overlay.moveCursor",
      "overlay.moveCursorById",
      "overlay.flashCursor",
      "overlay.flashCursorById",
      "overlay.hideCursorId",
      "overlay.hideCursor",
      "overlay.shutdown",
      "mediaControl.nowPlaying",
      "mediaControl.play",
      "mediaControl.pause",
      "mediaControl.togglePlayPause",
      "mediaControl.next",
      "mediaControl.previous",
      "steam.library.list.v0",
      "fixture.observe",
    ] {
      assert!(registry.resolve(id).is_some(), "{id}");
    }
  }

  #[test]
  fn media_control_commands_advertise_state_and_verification_signals() {
    let registry = default_registry();
    for id in [
      "mediaControl.nowPlaying",
      "mediaControl.play",
      "mediaControl.pause",
      "mediaControl.togglePlayPause",
      "mediaControl.next",
      "mediaControl.previous",
    ] {
      let command = registry.resolve(id).expect("media command should exist");
      for signal in [
        "mediaControl.present",
        "mediaControl.title",
        "mediaControl.isPlaying",
        "mediaControl.sourceBundleId",
        "mediaControl.verification",
      ] {
        assert!(command.signals.contains(&signal), "{id} missing {signal}");
      }
    }
  }

  #[test]
  fn media_transport_commands_are_mutating_actions() {
    let registry = default_registry();
    for id in [
      "mediaControl.play",
      "mediaControl.pause",
      "mediaControl.togglePlayPause",
      "mediaControl.next",
      "mediaControl.previous",
    ] {
      let command = registry.resolve(id).expect("media command should exist");
      assert_eq!(
        command.operation.namespace,
        OperationNamespace::Action,
        "{id}"
      );
      assert_eq!(
        command.operation.max_disturbance,
        OperationDisturbance::Keyboard,
        "{id}"
      );
    }
  }

  #[test]
  fn default_registry_has_no_legacy_id_prefixes() {
    let registry = default_registry();
    for command in registry.all() {
      assert!(
        !command.operation.id.starts_with("debug."),
        "{} should not use debug.*",
        command.operation.id
      );
      assert!(
        !command.operation.id.starts_with("verify."),
        "{} should not use verify.*",
        command.operation.id
      );
      assert!(
        !command.operation.id.starts_with("music."),
        "{} should not use music.*",
        command.operation.id
      );
    }
  }
}
