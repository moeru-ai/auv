use std::str::FromStr;

use auv_cli_invoke::{InvokeResult, default_registry};
use auv_tracing::RunId;

#[test]
fn failed_invoke_does_not_advertise_the_retired_inspect_command() {
  let registry = default_registry();
  let command = registry.resolve("scan.frame").expect("scan command");
  let run_id = RunId::from_str("019f8b1e-4b2d-7a00-8f00-0000000000aa").expect("run id");
  let result = InvokeResult::from_command_result(run_id, command, Err("fixture failed".to_string()));

  let output = result.render_to_string(Default::default()).expect("render should succeed");

  assert!(output.contains("fixture failed"));
  assert!(!output.contains("auv inspect"));
  assert!(!output.contains("Inspect:"));
}
