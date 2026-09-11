use auv_cli::cli::help_text;
use auv_cli_invoke::InvokeCliParse;

fn arguments(values: &[&str]) -> Vec<String> {
  values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn invoke_help_is_available_without_legacy_inspect_surface() {
  let command = auv_cli_invoke::parse_invoke_args(&arguments(&["invoke", "--help"])).expect("invoke help should parse");
  assert!(matches!(command, InvokeCliParse::Help { command_id: None }));
  assert!(!help_text().contains("auv inspect"));
  assert!(!help_text().contains("--inspect-server"));
}

#[test]
fn invoke_store_root_configures_tracing_without_becoming_command_input() {
  let command = auv_cli_invoke::parse_invoke_args(&arguments(&[
    "invoke",
    "scan.frame",
    "--fixture-dir",
    "unused",
    "--store-root",
    "trace-output",
    "--label",
    "fixture",
  ]))
  .expect("invoke should parse");
  let InvokeCliParse::Invoke {
    inputs, store_root, ..
  } = command
  else {
    panic!("expected invoke command");
  };
  assert_eq!(store_root.as_deref(), Some(std::path::Path::new("trace-output")));
  assert_eq!(inputs.get("label").map(String::as_str), Some("fixture"));
  assert!(!inputs.contains_key("store-root"));
}

#[test]
fn screen_find_text_accepts_a_typed_positional_query() {
  let command = auv_cli_invoke::parse_invoke_args(&arguments(&[
    "invoke",
    "screen.findText",
    "Settings",
    "--target",
    "com.apple.TextEdit",
    "--dry-run",
  ]))
  .expect("typed invoke command should parse");
  let InvokeCliParse::Invoke {
    inputs,
    target,
    dry_run,
    ..
  } = command
  else {
    panic!("expected invoke command");
  };

  assert_eq!(inputs.get("query").map(String::as_str), Some("Settings"));
  assert_eq!(
    target,
    Some(auv_cli_invoke::ExecutionTarget::Application {
      id: "com.apple.TextEdit".to_string(),
    })
  );
  assert!(dry_run);
}

#[test]
fn invoke_context_uses_clap_equals_and_end_of_options_semantics() {
  let command = auv_cli_invoke::parse_invoke_args(&arguments(&[
    "invoke",
    "screen.findText",
    "Settings",
    "--target=com.apple.TextEdit",
    "--dry-run",
  ]))
  .expect("equals syntax should parse");
  let InvokeCliParse::Invoke {
    target, dry_run, ..
  } = command
  else {
    panic!("expected invoke command");
  };
  assert_eq!(
    target,
    Some(auv_cli_invoke::ExecutionTarget::Application {
      id: "com.apple.TextEdit".to_string(),
    })
  );
  assert!(dry_run);

  let command =
    auv_cli_invoke::parse_invoke_args(&arguments(&["invoke", "input.typeText", "--", "--dry-run"])).expect("literal flag text should parse");
  let InvokeCliParse::Invoke {
    inputs, dry_run, ..
  } = command
  else {
    panic!("expected invoke command");
  };
  assert_eq!(inputs.get("text").map(String::as_str), Some("--dry-run"));
  assert!(!dry_run);
}

#[test]
fn unknown_top_level_names_are_reserved_for_external_plugins() {
  let output = std::process::Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["inspect", "019f8b1e-4b2d-7a00-8f00-0000000000aa"])
    .env("PATH", "")
    .output()
    .expect("route external command");
  assert!(!output.status.success());
  assert!(String::from_utf8_lossy(&output.stderr).contains("auv-inspect"));
}

#[test]
fn doctor_can_explicitly_request_missing_permissions() {
  let command = parse_cli(&arguments(&["doctor", "--json", "--request-permissions"])).expect("doctor should parse");
  let CliCommand::PermissionCheck {
    json,
    request_permissions,
  } = command
  else {
    panic!("expected permission check command");
  };

  assert!(json);
  assert!(request_permissions);
}
