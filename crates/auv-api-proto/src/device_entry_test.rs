use super::*;

#[test]
fn every_reason_round_trips_through_a_status() {
  use DeviceEntryErrorReason as Reason;

  for reason in [
    Reason::Unauthorized,
    Reason::Disabled,
    Reason::Unenrolled,
    Reason::Suspended,
    Reason::AmbiguousUser,
    Reason::StaleSession,
    Reason::OccupiedDesktop,
    Reason::UnsupportedOsState,
    Reason::ServiceUnavailable,
    Reason::CredentialRejected,
    Reason::OutcomeUnverified,
    Reason::AuditUnavailable,
    Reason::HostIncompatible,
  ] {
    let status = status(reason);

    assert_eq!(status.code(), code(reason), "{reason:?}");
    assert_eq!(super::reason(&status), Some(reason), "{reason:?}");
    assert!(status.message().contains(reason.as_str_name()), "{}", status.message());
  }
}

#[test]
fn status_codes_follow_the_reason() {
  use DeviceEntryErrorReason as Reason;

  assert_eq!(code(Reason::Disabled), tonic::Code::PermissionDenied);
  assert_eq!(code(Reason::Unenrolled), tonic::Code::FailedPrecondition);
  assert_eq!(code(Reason::AmbiguousUser), tonic::Code::InvalidArgument);
  assert_eq!(code(Reason::StaleSession), tonic::Code::Aborted);
  assert_eq!(code(Reason::ServiceUnavailable), tonic::Code::Unavailable);
  assert_eq!(code(Reason::OutcomeUnverified), tonic::Code::Unknown);
}

#[test]
fn the_details_are_a_standard_google_rpc_status() {
  // Generic gRPC tooling decodes `grpc-status-details-bin` as
  // `google.rpc.Status`; the typed reason is one `Any` inside it.
  let status = status(DeviceEntryErrorReason::Unenrolled);
  let envelope = RpcStatus::decode(status.details()).unwrap();

  assert_eq!(envelope.code, tonic::Code::FailedPrecondition as i32);
  assert_eq!(envelope.details.len(), 1);
  assert_eq!(envelope.details[0].type_url, "type.googleapis.com/auv.api.daemon.v1.DeviceEntryError");
}

#[test]
fn other_failures_carry_no_reason() {
  assert_eq!(reason(&tonic::Status::invalid_argument("session_selector is required")), None);

  // A detail of another type is not mistaken for an entry reason.
  let foreign = RpcStatus {
    code: tonic::Code::Internal as i32,
    message: "other".into(),
    details: vec![prost_types::Any {
      type_url: "type.googleapis.com/auv.api.driver.v1.KeyboardInputProgress".into(),
      value: vec![8, 1],
    }],
  };
  let status = tonic::Status::with_details(tonic::Code::Internal, "other", foreign.encode_to_vec().into());
  assert_eq!(reason(&status), None);

  assert_eq!(reason(&tonic::Status::with_details(tonic::Code::Internal, "garbage", vec![0xff, 0xff].into())), None);
}

/// Shared with the JS SDK test `decodes a typed reason from the status details`,
/// so both sides agree on the exact bytes.
const UNENROLLED_STATUS_DETAILS: &str = "0809123944657669636520656e747279206661696c65643a204445564943455f454e5452595f4552524f525f524541534f4e5f554e454e524f4c4c45441a3c0a36747970652e676f6f676c65617069732e636f6d2f6175762e6170692e6461656d6f6e2e76312e446576696365456e7472794572726f7212020802";

#[test]
fn unenrolled_status_matches_the_shared_fixture() {
  let status = status(DeviceEntryErrorReason::Unenrolled);
  let hex = status.details().iter().map(|byte| format!("{byte:02x}")).collect::<String>();

  assert_eq!(hex, UNENROLLED_STATUS_DETAILS);
}
