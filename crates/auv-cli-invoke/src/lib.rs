//! Registry-backed CLI invoke metadata and help rendering.
//!
//! This crate owns how invoke-visible commands are described, grouped, and
//! parsed for `auv invoke ...`.

use std::collections::BTreeMap;

use clap::{Arg, ArgAction, Command};

extern crate self as auv_cli_invoke;

pub mod arg;
pub mod command;
pub mod commands;
pub mod help;
pub mod model;
pub mod recorded;
pub mod registry;
pub mod render;
pub mod summary;

pub use arg::ArgSpec;
pub use auv_cli_invoke_macros::invoke_command;
pub use command::{CommandGroup, CommandNode, InvokeCommand, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeNamespace};
pub use help::{render_command_help, render_help_index};
pub use model::{
  ExecutionTarget, InvokeOutputOptions, InvokeReport, InvokeReportField, InvokeReportSection, InvokeReportTable, InvokeReportTableRow,
  InvokeRequest, InvokeResult, RunStatus,
};
pub use recorded::{
  InvokeFinalizeHook, invoke_recorded, invoke_recorded_in_span, invoke_recorded_with_finalize, invoke_recorded_with_session,
  invoke_resolved_recorded_in_span,
};
pub use registry::{InvokeRegistry, default_registry};
pub use render::{render_invoke_result, render_to_string, write_rendered};
pub use summary::{OperationSummary, OperationSummaryCache, OperationSummaryRecord, OperationSummarySource};

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
    output: InvokeOutputOptions,
  },
}

pub fn parse_invoke_args(arguments: &[String]) -> Result<InvokeCliParse, String> {
  let tokens = normalize_invoke_arguments(arguments);
  if tokens.is_empty() {
    return Ok(InvokeCliParse::Help { command_id: None });
  }

  if tokens.is_empty() || tokens.first().is_some_and(|token| token == "help") {
    return Ok(InvokeCliParse::Help { command_id: None });
  }

  let normalized = normalize_for_clap(&tokens)?;
  if let Some(help) = normalized.help {
    return Ok(help);
  }

  let matches = invoke_cli_command().try_get_matches_from(normalized.clap_arguments).map_err(|error| error.to_string())?;
  let command_id = matches.get_one::<String>("command_id").cloned().ok_or_else(|| "missing invoke command id".to_string())?;
  let mut inputs = normalized.inputs;
  if let Some(label) = matches.get_one::<String>("label") {
    inputs.insert("label".to_string(), label.clone());
  }

  Ok(InvokeCliParse::Invoke {
    command_id,
    target_application_id: matches.get_one::<String>("target").cloned(),
    inputs,
    dry_run: matches.get_flag("dry_run"),
    output: InvokeOutputOptions {
      json: matches.get_flag("json") || matches.get_flag("format"),
      detail: matches.get_flag("detail"),
      wide: matches.get_flag("wide"),
    },
  })
}

pub fn invoke_argument_consumes_value(argument: &str) -> bool {
  match argument {
    "--dry-run" | "--detail" | "--wide" | "--json" | "--format" | "--help" | "-h" => false,
    other => other.starts_with("--"),
  }
}

struct NormalizedInvokeArguments {
  clap_arguments: Vec<String>,
  inputs: BTreeMap<String, String>,
  help: Option<InvokeCliParse>,
}

fn normalize_invoke_arguments(arguments: &[String]) -> Vec<String> {
  match arguments.first().map(String::as_str) {
    Some("invoke") => arguments.iter().skip(1).cloned().collect(),
    _ => arguments.to_vec(),
  }
}

fn invoke_cli_command() -> Command {
  Command::new("invoke")
    .disable_help_flag(true)
    .arg(Arg::new("command_id").index(1).value_name("command-id"))
    .arg(Arg::new("dry_run").long("dry-run").action(ArgAction::SetTrue))
    .arg(Arg::new("target").long("target").value_name("bundle-id").num_args(1))
    .arg(Arg::new("label").long("label").value_name("value").num_args(1))
    .arg(Arg::new("detail").long("detail").action(ArgAction::SetTrue))
    .arg(Arg::new("wide").long("wide").action(ArgAction::SetTrue))
    .arg(Arg::new("json").long("json").action(ArgAction::SetTrue))
    .arg(Arg::new("format").long("format").action(ArgAction::SetTrue).hide(true))
}

fn normalize_for_clap(tokens: &[String]) -> Result<NormalizedInvokeArguments, String> {
  let mut clap_arguments = vec!["invoke".to_string()];
  let mut inputs = BTreeMap::new();
  let mut command_id = None;
  let mut index = 0;

  while index < tokens.len() {
    let token = &tokens[index];
    match token.as_str() {
      "--help" | "-h" => {
        return Ok(NormalizedInvokeArguments {
          clap_arguments,
          inputs,
          help: Some(InvokeCliParse::Help { command_id }),
        });
      }
      "--dry-run" | "--detail" | "--wide" | "--json" | "--format" => {
        clap_arguments.push(token.clone());
        index += 1;
      }
      "--target" | "--label" => {
        clap_arguments.push(token.clone());
        if let Some(value) = tokens.get(index + 1) {
          clap_arguments.push(value.clone());
          index += 2;
        } else {
          index += 1;
        }
      }
      flag if flag.starts_with("--") => {
        let Some(value) = tokens.get(index + 1) else {
          return Err(format!("flag {flag} requires a value"));
        };
        let key = flag.trim_start_matches("--");
        inputs.insert(key.to_string(), value.clone());
        index += 2;
      }
      positional => {
        if command_id.is_none() {
          command_id = Some(positional.to_string());
          clap_arguments.push(positional.to_string());
          index += 1;
        } else {
          return Err(format!("unexpected positional argument {positional}"));
        }
      }
    }
  }

  Ok(NormalizedInvokeArguments {
    clap_arguments,
    inputs,
    help: None,
  })
}

#[cfg(test)]
mod tests {
  use super::{
    CommandGroup, InvokeNamespace, InvokeOutputOptions, InvokeRegistry, default_registry, invoke_cli_command, render_command_help,
    render_help_index,
  };

  #[test]
  fn help_index_groups_commands_by_namespace() {
    let registry = default_registry();
    let help = render_help_index(&registry);

    assert!(help.contains("DISPLAY\n"));
    assert!(help.contains("--json"));
    assert!(help.contains("--detail"));
    assert!(help.contains("--wide"));
    assert!(!help.contains("--format"));
    assert!(help.contains("  display.list"));
    assert!(help.contains("WINDOW\n"));
    assert!(help.contains("  window.capture"));
    assert!(help.contains("INPUT\n"));
    assert!(help.contains("  input.key"));
    assert!(help.contains("MEDIA CONTROL\n"));
    assert!(help.contains("  mediaControl.nowPlaying"));
    assert!(help.contains("SCAN\n"));
    assert!(help.contains("  scan.frame"));
    assert!(help.contains("  scan.coverage"));
    assert!(!help.contains("STEAM\n"));
    assert!(!help.contains("  steam.library.list.v0"));
    assert!(!help.contains("debug."));
    assert!(!help.contains("verify."));
    assert!(!help.contains("music."));
  }

  #[test]
  fn registry_supports_nested_command_groups() {
    let fixture = default_registry().resolve("fixture.observe").expect("fixture command should exist").clone();
    let registry =
      InvokeRegistry::from_groups(vec![CommandGroup::new("root", "ROOT").group(CommandGroup::new("child", "CHILD").command(fixture))]);

    assert!(registry.resolve("fixture.observe").is_some());
    let help = render_help_index(&registry);
    assert!(help.contains("ROOT\n"));
    assert!(help.contains("  CHILD\n"));
    assert!(help.contains("    fixture.observe"));
  }

  #[test]
  #[should_panic(expected = "duplicate invoke command id registered: fixture.observe")]
  fn registry_rejects_duplicate_command_ids() {
    let fixture = default_registry().resolve("fixture.observe").expect("fixture command should exist").clone();

    let _registry = InvokeRegistry::from_groups(vec![
      CommandGroup::new("one", "ONE").command(fixture.clone()),
      CommandGroup::new("two", "TWO").command(fixture),
    ]);
  }

  #[test]
  fn command_metadata_preserves_invoke_surface() {
    let registry = default_registry();
    let command = registry.resolve("fixture.observe").expect("fixture.observe should be registered");

    assert_eq!(command.id, "fixture.observe");
    assert_eq!(command.namespace, InvokeNamespace::Fixture);
    assert_eq!(command.summary, "Emit a deterministic observation result without touching the real UI.");
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
        output,
      } => {
        assert_eq!(command_id, "input.key");
        assert_eq!(target_application_id.as_deref(), Some("com.example.App"));
        assert!(dry_run);
        assert_eq!(output, InvokeOutputOptions::default());
        assert_eq!(inputs.get("label").map(String::as_str), Some("smoke"));
        assert_eq!(inputs.get("key").map(String::as_str), Some("Return"));
      }
      other => panic!("unexpected parse result: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_json_before_and_after_command_match() {
    let before = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "--json".to_string(),
      "display.list".to_string(),
    ])
    .expect("json before command should parse");
    let after = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "display.list".to_string(),
      "--json".to_string(),
    ])
    .expect("json after command should parse");

    assert_eq!(before, after);
  }

  #[test]
  fn parse_invoke_detail_keeps_human_output_mode() {
    let parsed = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "display.list".to_string(),
      "--detail".to_string(),
    ])
    .expect("detail should parse");

    match parsed {
      crate::InvokeCliParse::Invoke { output, .. } => {
        assert!(output.detail);
        assert!(!output.json);
      }
      other => panic!("unexpected parse result: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_wide_keeps_human_output_mode_and_does_not_become_command_input() {
    let parsed = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "window.list".to_string(),
      "--wide".to_string(),
    ])
    .expect("wide should parse");

    match parsed {
      crate::InvokeCliParse::Invoke { inputs, output, .. } => {
        assert!(output.wide);
        assert!(!output.json);
        assert!(!output.detail);
        assert!(!inputs.contains_key("wide"));
      }
      other => panic!("unexpected parse result: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_format_bare_alias_matches_json() {
    let format = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "display.list".to_string(),
      "--format".to_string(),
    ])
    .expect("bare format alias should parse");
    let json = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "display.list".to_string(),
      "--json".to_string(),
    ])
    .expect("json should parse");

    assert_eq!(format, json);
  }

  #[test]
  fn parse_invoke_help_hides_format_alias() {
    let help = invoke_cli_command().render_help().to_string();

    assert!(help.contains("--json"));
    assert!(!help.contains("--format"));
  }

  #[test]
  fn parse_invoke_preserves_known_and_unknown_command_inputs() {
    let parsed = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "input.key".to_string(),
      "--label".to_string(),
      "Foo".to_string(),
      "--key".to_string(),
      "Cmd+L".to_string(),
      "--settle_ms".to_string(),
      "250".to_string(),
    ])
    .expect("command inputs should parse");

    match parsed {
      crate::InvokeCliParse::Invoke { inputs, .. } => {
        assert_eq!(inputs.get("label").map(String::as_str), Some("Foo"));
        assert_eq!(inputs.get("key").map(String::as_str), Some("Cmd+L"));
        assert_eq!(inputs.get("settle_ms").map(String::as_str), Some("250"));
      }
      other => panic!("unexpected parse result: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_target_and_dry_run_keep_existing_behavior() {
    let parsed = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "input.key".to_string(),
      "--dry-run".to_string(),
      "--target".to_string(),
      "com.example.App".to_string(),
    ])
    .expect("target and dry-run should parse");

    match parsed {
      crate::InvokeCliParse::Invoke {
        target_application_id,
        dry_run,
        ..
      } => {
        assert!(dry_run);
        assert_eq!(target_application_id.as_deref(), Some("com.example.App"));
      }
      other => panic!("unexpected parse result: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_help_forms_preserve_existing_behavior() {
    let index_help = crate::parse_invoke_args(&["invoke".to_string(), "--help".to_string()]).expect("invoke --help should parse");
    let help_command = crate::parse_invoke_args(&["invoke".to_string(), "help".to_string()]).expect("invoke help should parse");
    let command_help = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "window.capture".to_string(),
      "--help".to_string(),
    ])
    .expect("invoke command help should parse");

    assert_eq!(index_help, crate::InvokeCliParse::Help { command_id: None });
    assert_eq!(help_command, crate::InvokeCliParse::Help { command_id: None });
    assert_eq!(
      command_help,
      crate::InvokeCliParse::Help {
        command_id: Some("window.capture".to_string())
      }
    );
  }

  #[test]
  fn parse_invoke_unknown_flags_become_inputs() {
    let parsed = crate::parse_invoke_args(&[
      "invoke".to_string(),
      "input.typeText".to_string(),
      "--text".to_string(),
      "hello".to_string(),
      "--replace_existing".to_string(),
      "true".to_string(),
    ])
    .expect("unknown invoke inputs should parse");

    match parsed {
      crate::InvokeCliParse::Invoke { inputs, .. } => {
        assert_eq!(inputs.get("text").map(String::as_str), Some("hello"));
        assert_eq!(inputs.get("replace_existing").map(String::as_str), Some("true"));
      }
      other => panic!("unexpected parse result: {other:?}"),
    }
  }

  #[test]
  fn command_help_renders_metadata_only_sections() {
    let registry = default_registry();
    let command = registry.resolve("fixture.observe").expect("fixture.observe should be registered");

    let help = render_command_help(command);

    assert!(help.contains("COMMAND\n  fixture.observe"));
    assert!(help.contains("USAGE\n  auv invoke fixture.observe"));
    assert!(help.contains("SUMMARY\n  Emit a deterministic observation result"));
    assert!(help.contains("OPTIONS\n  --json"));
    assert!(help.contains("--detail"));
    assert!(help.contains("--wide"));
    assert!(!help.contains("OPTIONS\n  none"));
    assert!(!help.contains("DRIVER\n"));
    assert!(!help.contains("DISTURBANCE\n"));
    assert!(!help.contains("ARTIFACTS\n"));
    assert!(!help.contains("SIGNALS\n"));
    assert!(!help.contains("VERIFY\n"));
  }

  #[test]
  fn help_index_skips_empty_nested_groups() {
    let fixture = default_registry().resolve("fixture.observe").expect("fixture command should exist").clone();
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
      "scan.frame",
      "scan.coverage",
    ] {
      assert!(registry.resolve(id).is_some(), "{id} should be registered");
    }
    assert!(registry.resolve("steam.library.list.v0").is_none());
  }
}
