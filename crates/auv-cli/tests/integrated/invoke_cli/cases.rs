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
  assert_eq!(tracing.store_root.as_deref(), Some("trace-output"));
  assert_eq!(request.inputs.get("label").map(String::as_str), Some("fixture"));
  assert!(!request.inputs.contains_key("store-root"));
}

#[test]
fn inspect_command_reports_the_explicit_retirement_boundary() {
  let error = parse_cli(&arguments(&["inspect", "019f8b1e-4b2d-7a00-8f00-0000000000aa"])).expect_err("inspect is retired");
  assert!(error.contains("has been retired"));
  assert!(error.contains("inspector read-side"));
}
