use auv_cli::cli::{CliCommand, help_text, parse_cli};

fn arguments(values: &[&str]) -> Vec<String> {
  values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn invoke_help_is_available_without_legacy_inspect_surface() {
  let command = parse_cli(&arguments(&["invoke", "--help"])).expect("invoke help should parse");
  assert!(matches!(command, CliCommand::InvokeHelp { command_id: None }));
  assert!(!help_text().contains("auv inspect"));
  assert!(!help_text().contains("--inspect-server"));
}

#[test]
fn invoke_store_root_configures_tracing_without_becoming_command_input() {
  let command = parse_cli(&arguments(&[
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
  let CliCommand::Invoke {
    request, tracing, ..
  } = command
  else {
    panic!("expected invoke command");
  };
  assert_eq!(tracing.store_root.as_deref(), Some(std::path::Path::new("trace-output")));
  assert_eq!(request.inputs.get("label").map(String::as_str), Some("fixture"));
  assert!(!request.inputs.contains_key("store-root"));
}

#[test]
fn screen_find_text_accepts_a_typed_positional_query() {
  let command = parse_cli(&arguments(&[
    "invoke",
    "screen.findText",
    "Settings",
    "--target",
    "com.apple.TextEdit",
    "--dry-run",
  ]))
  .expect("typed invoke command should parse");
  let CliCommand::Invoke { request, .. } = command else {
    panic!("expected invoke command");
  };

  assert_eq!(request.inputs.get("query").map(String::as_str), Some("Settings"));
  assert_eq!(request.target.application_id.as_deref(), Some("com.apple.TextEdit"));
  assert!(request.dry_run);
}

#[test]
fn invoke_context_uses_clap_equals_and_end_of_options_semantics() {
  let command = parse_cli(&arguments(&[
    "invoke",
    "screen.findText",
    "Settings",
    "--target=com.apple.TextEdit",
    "--dry-run",
  ]))
  .expect("equals syntax should parse");
  let CliCommand::Invoke { request, .. } = command else {
    panic!("expected invoke command");
  };
  assert_eq!(request.target.application_id.as_deref(), Some("com.apple.TextEdit"));
  assert!(request.dry_run);

  let command = parse_cli(&arguments(&["invoke", "input.typeText", "--", "--dry-run"])).expect("literal flag text should parse");
  let CliCommand::Invoke { request, .. } = command else {
    panic!("expected invoke command");
  };
  assert_eq!(request.inputs.get("text").map(String::as_str), Some("--dry-run"));
  assert!(!request.dry_run);
}

#[test]
fn unknown_top_level_names_are_reserved_for_external_plugins() {
  let command = parse_cli(&arguments(&["inspect", "019f8b1e-4b2d-7a00-8f00-0000000000aa"])).expect("external command should parse");
  assert!(matches!(command, CliCommand::External { .. }));
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
