use auv_cli_invoke::{InvokeCancellation, InvokeCommandOutput};

#[test]
fn cancellation_is_cloneable_and_observable_through_the_public_api() {
  let cancellation = InvokeCancellation::new();
  let observer = cancellation.clone();

  assert!(observer.check().is_ok());
  cancellation.cancel();

  let error = observer.check().expect_err("shared cancellation must be observable");
  assert_eq!(error.to_string(), "invoke cancelled");
}

#[test]
fn completed_command_output_has_no_generic_report() {
  let output = InvokeCommandOutput::completed();

  assert!(output.report.is_none());
}
