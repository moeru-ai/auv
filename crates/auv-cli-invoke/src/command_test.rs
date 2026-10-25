use super::*;

// ROOT CAUSE:
//
// If a Runner-route capability or placement RPC failed, the selected-Runner
// arms stringified the error into "RPC failed: {error}", and From<String>
// classified the result as command_failed. The transport category carried by
// ClientErrorKind (tonic NOT_FOUND, UNIMPLEMENTED, PERMISSION_DENIED,
// INVALID_ARGUMENT, FAILED_PRECONDITION, ...) was dropped, so CLI and MCP
// callers could not branch on the failure code.
//
// Before the fix, every Runner-route RPC failure reported command_failed.
// The fix keeps one shared classification, client_failure_code, for every
// error on the route that carries a transport category.
#[test]
fn client_failure_codes_pin_the_transport_category_classification() {
  use auv::error::ClientErrorKind;
  let cases = [
    (ClientErrorKind::NotFound, FailureCode::NotFound),
    (ClientErrorKind::Unsupported, FailureCode::Unsupported),
    (ClientErrorKind::Unauthorized, FailureCode::PermissionDenied),
    (ClientErrorKind::InvalidRequest, FailureCode::InvalidInput),
    (ClientErrorKind::Conflict, FailureCode::StaleUiReference),
    (ClientErrorKind::Ambiguous, FailureCode::StaleUiReference),
    (ClientErrorKind::Unavailable, FailureCode::Backend),
    (ClientErrorKind::Protocol, FailureCode::Backend),
  ];
  for (kind, code) in cases {
    assert_eq!(client_failure_code(Some(kind)), code, "{kind:?}");
  }
  assert_eq!(client_failure_code(None), FailureCode::Backend);
}

#[test]
fn capability_failures_reuse_the_transport_classification() {
  // CapabilityError::NotFound is the client-side derived not-found, such as
  // an unknown Runner hold id.
  let failure = InvokeFailure::from(auv::client::runner::CapabilityError::NotFound("WindowsClient::get 777".to_string()));
  assert_eq!(failure.code, FailureCode::NotFound);
  assert_eq!(failure.message, "WindowsClient::get 777 was not found");

  // Client-side validation and contract violations carry no transport
  // category, so they classify no more specifically than backend.
  let invalid =
    InvokeFailure::from(auv::client::runner::CapabilityError::InvalidArgument("hold duration must be within (0, 30s]".to_string()));
  assert_eq!(invalid.code, FailureCode::Backend);
  assert_eq!(invalid.message, "Runner request is invalid: hold duration must be within (0, 30s]");
  let malformed = InvokeFailure::from(auv::client::runner::CapabilityError::InvalidResponse("key hold id missing".to_string()));
  assert_eq!(malformed.code, FailureCode::Backend);
  assert_eq!(malformed.message, "Runner response is invalid: key hold id missing");
}

// TODO(runner-failure-roundtrip-tests): CapabilityError::Client and
// ::KeyboardInput wrap auv-core's ClientError, which cannot be constructed
// outside auv-core, and RunnerClient::new is pub(crate), so this crate cannot
// exercise the tonic-status round trip or the keyboard-progress copy in
// From<CapabilityError>. That branch is unchanged from the owner-approved
// conversion; pin it end to end once an owner-approved loopback fixture or
// test seam exists.
#[test]
fn placement_failures_classify_transport_categories_and_keep_deferred_text() {
  // Placement re-surfaces capability failures; the shared conversion keeps
  // their classification and message.
  let capability =
    InvokeFailure::from(auv::client::PlacementError::Capability(auv::client::runner::CapabilityError::NotFound("hold id 777".to_string())));
  assert_eq!(capability.code, FailureCode::NotFound);
  assert_eq!(capability.message, "hold id 777 was not found");

  // Free-form placement failures keep their legacy command_failed class.
  let selection = InvokeFailure::from(auv::client::PlacementError::Selection("unknown Run ID \"abc\"".to_string()));
  assert_eq!(selection.code, FailureCode::CommandFailed);
  assert_eq!(selection.message, "unknown Run ID \"abc\"");
  let environment = InvokeFailure::from(auv::client::PlacementError::ContextEnvironment(std::env::VarError::NotPresent));
  assert_eq!(environment.code, FailureCode::CommandFailed);
  assert_eq!(environment.message, "AUV_CONTEXT is not valid Unicode: environment variable not found");
  let cleanup = InvokeFailure::from(auv::client::PlacementError::Cleanup {
    primary: "route core Runner failed".to_string(),
    cleanup: "cancel failed".to_string(),
  });
  assert_eq!(cleanup.code, FailureCode::CommandFailed);
  assert_eq!(cleanup.message, "route core Runner failed; cleanup also failed: cancel failed");
}
