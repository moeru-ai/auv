//! Registry-backed CLI invoke metadata and help rendering.
//!
//! This crate owns how invoke-visible commands are described, grouped, and
//! parsed for `auv-cli invoke ...`.

use std::collections::BTreeMap;

extern crate self as auv_cli_invoke;

pub mod arg;
pub mod command;
pub mod commands;
pub mod help;
pub mod model;
pub mod recorded;
pub mod registry;

pub use arg::ArgSpec;
pub use auv_cli_invoke_macros::invoke_command;
pub use command::{
  CommandGroup, CommandNode, InvokeCommand, InvokeCommandInput, InvokeCommandOutput,
  InvokeCommandResult, InvokeNamespace,
};
pub use help::{render_command_help, render_help_index};
pub use model::{ExecutionTarget, InvokeRequest, InvokeResult, RunStatus};
pub use recorded::{invoke_recorded, invoke_recorded_in_span, invoke_resolved_recorded_in_span};
pub use registry::{InvokeRegistry, default_registry};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvokeCliParse {
  Help {
    command_id: Option<String>,
  },
  Invoke {
    command_id: String,
    target_application_id: Option<String>,
    inputs: BTreeMap<String, String>,
    dry_run: bool,
  },
}

pub fn parse_invoke_args(arguments: &[String]) -> Result<InvokeCliParse, String> {
  if arguments.len() < 2 {
    return Ok(InvokeCliParse::Help { command_id: None });
  }

  if matches!(arguments[1].as_str(), "--help" | "-h" | "help") {
    return Ok(InvokeCliParse::Help { command_id: None });
  }

  let command_id = arguments[1].clone();
  if arguments.len() == 3 && matches!(arguments[2].as_str(), "--help" | "-h") {
    return Ok(InvokeCliParse::Help {
      command_id: Some(command_id),
    });
  }

  let mut target_application_id = None;
  let mut inputs = BTreeMap::new();
  let mut dry_run = false;
  let mut index = 2;

  while index < arguments.len() {
    let argument = &arguments[index];
    if !argument.starts_with("--") {
      return Err(format!("unexpected positional argument {argument}"));
    }

    if argument == "--dry-run" {
      dry_run = true;
      index += 1;
      continue;
    }

    if index + 1 >= arguments.len() {
      return Err(format!("flag {argument} requires a value"));
    }

    let value = arguments[index + 1].clone();
    match argument.as_str() {
      "--target" => {
        target_application_id = Some(value);
      }
      "--label" => {
        inputs.insert("label".to_string(), value);
      }
      other => {
        let key = other.trim_start_matches("--");
        inputs.insert(key.to_string(), value);
      }
    }

    index += 2;
  }

  Ok(InvokeCliParse::Invoke {
    command_id,
    target_application_id,
    inputs,
    dry_run,
  })
}

#[cfg(test)]
mod tests {
  use super::{
    CommandGroup, InvokeNamespace, InvokeRegistry, default_registry, render_command_help,
    render_help_index,
  };

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
    assert!(!help.contains("STEAM\n"));
    assert!(!help.contains("  steam.library.list.v0"));
    assert!(!help.contains("debug."));
    assert!(!help.contains("verify."));
    assert!(!help.contains("music."));
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
  }

  #[test]
  fn parse_invoke_args_preserves_invoke_surface() {
    let parsed = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "input.key".to_string(),
      "--dry-run".to_string(),
      "--target".to_string(),
      "com.example.App".to_string(),
      "--label".to_string(),
      "smoke".to_string(),
      "--key".to_string(),
      "Return".to_string(),
    ])
    .expect("invoke args should parse");

    match parsed {
      crate::InvokeCliParse::Invoke {
        command_id,
        target_application_id,
        inputs,
        dry_run,
      } => {
        assert_eq!(command_id, "input.key");
        assert_eq!(target_application_id.as_deref(), Some("com.example.App"));
        assert!(dry_run);
        assert_eq!(inputs.get("label").map(String::as_str), Some("smoke"));
        assert_eq!(inputs.get("key").map(String::as_str), Some("Return"));
      }
      other => panic!("unexpected parse result: {other:?}"),
    }
  }

  #[test]
  fn command_help_renders_metadata_only_sections() {
    let registry = default_registry();
    let command = registry
      .resolve("fixture.observe")
      .expect("fixture.observe should be registered");

    let help = render_command_help(command);

    assert!(help.contains("COMMAND\n  fixture.observe"));
    assert!(help.contains("USAGE\n  auv-cli invoke fixture.observe"));
    assert!(help.contains("SUMMARY\n  Emit a deterministic observation result"));
    assert!(help.contains("OPTIONS\n  none"));
    assert!(!help.contains("DRIVER\n"));
    assert!(!help.contains("DISTURBANCE\n"));
    assert!(!help.contains("ARTIFACTS\n"));
    assert!(!help.contains("SIGNALS\n"));
    assert!(!help.contains("VERIFY\n"));
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
      "fixture.observe",
    ] {
      assert!(registry.resolve(id).is_some(), "{id} should be registered");
    }
    assert!(registry.resolve("steam.library.list.v0").is_none());
  }
}
