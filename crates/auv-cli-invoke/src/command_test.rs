use super::*;

#[test]
fn click_precondition_failure_requires_recapture_across_frontends() {
  let failure = InvokeFailure::from(auv::client::runner::CapabilityError::CapturePrecondition {
    capture_id: "cap-test".into(),
    reason: auv::client::runner::ClickPreconditionFailureReason::StateChanged,
    cause_code: None,
    source: auv_api_proto::click_precondition::status(
      "cap-test",
      auv::client::runner::ClickPreconditionFailureReason::StateChanged,
      "captured pixels changed",
    ),
  });
  assert_eq!(failure.code, FailureCode::RecaptureRequired);
  let output = serde_json::to_value(&failure).unwrap();
  assert_eq!(output["code"], "recapture_required");
  assert!(failure.message.contains("take a new capture"));
  assert!(failure.message.contains("captured pixels changed"));
}

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

// Pins the serialized half of the runner failure roundtrip: every
// FailureCode a runner-route failure can carry reaches JSON output under its
// documented snake_case spelling with the step message intact, so a renamed
// or re-spelled variant cannot slip into callers' output silently.
#[test]
fn failure_codes_serialize_to_their_documented_json_spelling() {
  let cases = [
    (FailureCode::CommandFailed, "command_failed"),
    (FailureCode::InvalidTarget, "invalid_target"),
    (FailureCode::NotFound, "not_found"),
    (FailureCode::Unsupported, "unsupported"),
    (FailureCode::PermissionDenied, "permission_denied"),
    (FailureCode::InvalidInput, "invalid_input"),
    (FailureCode::StaleUiReference, "stale_ui_reference"),
    (FailureCode::RoleMismatch, "role_mismatch"),
    (FailureCode::Backend, "backend"),
  ];
  for (code, encoded) in cases {
    let failure = InvokeFailure::new(code, "InputService/GetPointerPosition failed: boom");
    let value = serde_json::to_value(&failure).expect("serialize failure");
    assert_eq!(value["code"], encoded, "{code:?}");
    assert_eq!(value["message"], "InputService/GetPointerPosition failed: boom");
  }
}

// TODO(runner-failure-roundtrip-tests): the transport half of the roundtrip
// is pinned hermetically — auv-core's loopback fixture serves the real tonic
// statuses and asserts each ClientErrorKind and its step context, the
// kind-to-code table is pinned above, and the JSON spelling of every code is
// pinned in the test above this marker. The remaining gap is the literal
// Client-wrapping From arms (CapabilityError::Client and ::KeyboardInput,
// PlacementError::Client, the two ContextError transport branches): they
// consume auv-core's ClientError, which cannot be constructed outside
// auv-core, and RunnerClient::new is pub(crate). They reuse the pinned
// client_failure_code; exercise them end to end here once an owner-approved
// loopback fixture or test seam exists.
#[test]
fn placement_failures_classify_transport_categories_and_keep_deferred_text() {
  // Placement re-surfaces capability failures; the shared conversion keeps
  // their classification and message.
  let capability =
    InvokeFailure::from(auv::client::PlacementError::Capability(auv::client::runner::CapabilityError::NotFound("hold id 777".to_string())));
  assert_eq!(capability.code, FailureCode::NotFound);
  assert_eq!(capability.message, "hold id 777 was not found");

  // Free-form placement and context failures keep their legacy command_failed
  // class and their full wording.
  let selection = InvokeFailure::from(auv::client::PlacementError::Selection("unknown Run ID \"abc\"".to_string()));
  assert_eq!(selection.code, FailureCode::CommandFailed);
  assert_eq!(selection.message, "unknown Run ID \"abc\"");
  let environment = InvokeFailure::from(auv::client::PlacementError::ContextEnvironment(std::env::VarError::NotPresent));
  assert_eq!(environment.code, FailureCode::CommandFailed);
  assert_eq!(environment.message, "AUV_CONTEXT is not valid Unicode: environment variable not found");
  let endpoint = InvokeFailure::from(auv::client::PlacementError::Context(auv::ContextError::EndpointNotDiscovered));
  assert_eq!(endpoint.code, FailureCode::CommandFailed);
  assert_eq!(endpoint.message, "could not resolve an AUV daemon endpoint from the context or local discovery");

  // A typed missing-Run fact classifies as not found; the wording is the
  // ContextError Display, unchanged from the legacy command_failed path.
  let run_missing = InvokeFailure::from(auv::client::PlacementError::Context(auv::ContextError::RunNotFound("run_123".to_string())));
  assert_eq!(run_missing.code, FailureCode::NotFound);
  assert_eq!(run_missing.message, "Run \"run_123\" was not found on the local daemon or any configured paired Device");

  // Ambiguous and unconfigured selections stay deferred: their multiple-
  // matches/configuration semantics have no clean current code (owner
  // decision, see TODO(runner-placement-failure-codes)).
  let run_ambiguous = InvokeFailure::from(auv::client::PlacementError::Context(auv::ContextError::RunAmbiguous {
    run_id: "run_123".to_string(),
    locations: "local, paired-laptop".to_string(),
  }));
  assert_eq!(run_ambiguous.code, FailureCode::CommandFailed);
  let device_ambiguous = InvokeFailure::from(auv::client::PlacementError::Context(auv::ContextError::DeviceSelectionAmbiguous {
    selector: "laptop".to_string(),
    candidate_ids: "dev-1, dev-2".to_string(),
  }));
  assert_eq!(device_ambiguous.code, FailureCode::CommandFailed);
  let device_unconfigured = InvokeFailure::from(auv::client::PlacementError::Context(auv::ContextError::DeviceNotConfigured));
  assert_eq!(device_unconfigured.code, FailureCode::CommandFailed);

  let cleanup = InvokeFailure::from(auv::client::PlacementError::Cleanup {
    primary: "route core Runner failed".to_string(),
    cleanup: "cancel failed".to_string(),
  });
  assert_eq!(cleanup.code, FailureCode::CommandFailed);
  assert_eq!(cleanup.message, "route core Runner failed; cleanup also failed: cancel failed");
}

// The local driver route carries keyboard partial progress through
// From<KeyboardInputError>; the Runner route performs the same copy in
// From<CapabilityError>, which this crate cannot construct (see the
// runner-failure-roundtrip-tests marker above). This pins the shared
// InvokeFailure.keyboard_progress contract on the constructible sibling.
#[test]
fn keyboard_input_failures_preserve_partial_progress() {
  let error = auv_driver::KeyboardInputError {
    cause: auv_driver::DriverError::Unsupported {
      operation: "type_text",
    },
    progress: auv_driver::KeyboardInputProgress {
      action_index: 2,
      completed: Vec::new(),
      completed_presses: 3,
    },
  };
  let failure = InvokeFailure::from(error);
  assert_eq!(failure.code, FailureCode::Unsupported);
  assert_eq!(
    failure.keyboard_progress,
    Some(auv_driver::KeyboardInputProgress {
      action_index: 2,
      completed: Vec::new(),
      completed_presses: 3,
    })
  );
  // The wrapper's own Display survives, naming the stopped action and the
  // completed work before the failure.
  assert!(failure.message.starts_with("keyboard action 2 failed after 0 complete actions and 3 presses: "));
  assert!(failure.message.ends_with("type_text is not supported by this driver"));
}
