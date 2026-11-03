//! Shared rich-error envelope for a failed native input action.

use prost::Message as _;

use crate::auv::api::driver::v1::InputActionFailure;
use crate::google::rpc::Status as RpcStatus;

const TYPE_URL: &str = "type.googleapis.com/auv.api.driver.v1.InputActionFailure";

/// Packs a domain-validated failure while retaining its gRPC category/message.
pub fn status(cause: &tonic::Status, failure: InputActionFailure) -> tonic::Status {
  if cause.code() == tonic::Code::Ok || failure.progress.is_none() {
    return tonic::Status::internal("input action failure requires a failed status and explicit progress");
  }
  let envelope = RpcStatus {
    code: cause.code() as i32,
    message: cause.message().into(),
    details: vec![prost_types::Any {
      type_url: TYPE_URL.into(),
      value: failure.encode_to_vec(),
    }],
  };
  tonic::Status::with_details(cause.code(), cause.message(), envelope.encode_to_vec().into())
}

/// Unknown, ambiguous, malformed, or unrelated details leave progress unknown.
/// Callers must also validate the domain action before treating it as evidence.
pub fn failure(status: &tonic::Status) -> Option<InputActionFailure> {
  if status.code() == tonic::Code::Ok {
    return None;
  }
  let envelope = RpcStatus::decode(status.details()).ok()?;
  if envelope.code != status.code() as i32 || envelope.message != status.message() {
    return None;
  }
  let mut matching = envelope.details.iter().filter(|detail| detail.type_url == TYPE_URL);
  let detail = matching.next()?;
  if matching.next().is_some() {
    return None;
  }
  let failure = InputActionFailure::decode(detail.value.as_slice()).ok()?;
  failure.progress.as_ref()?;
  Some(failure)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::auv::api::driver::v1::input_action_failure::Progress;

  #[test]
  fn an_explicit_zero_delivery_receipt_keeps_the_original_category() {
    let cause = tonic::Status::permission_denied("UIA permission denied");
    let encoded = status(
      &cause,
      InputActionFailure {
        progress: Some(Progress::NotDelivered(())),
      },
    );
    assert_eq!(encoded.code(), cause.code());
    assert_eq!(encoded.message(), cause.message());
    assert!(matches!(failure(&encoded).unwrap().progress, Some(Progress::NotDelivered(()))));
    let envelope = RpcStatus::decode(encoded.details()).unwrap();
    assert_eq!(envelope.details[0].type_url, TYPE_URL);
  }

  #[test]
  fn absent_unknown_and_malformed_details_never_become_zero_delivery() {
    let cause = tonic::Status::unavailable("provider readback failed");
    assert!(failure(&cause).is_none());
    for value in [vec![], vec![255], vec![0x22, 0x00]] {
      let envelope = RpcStatus {
        code: cause.code() as i32,
        message: cause.message().into(),
        details: vec![prost_types::Any {
          type_url: TYPE_URL.into(),
          value,
        }],
      };
      let encoded = tonic::Status::with_details(cause.code(), cause.message(), envelope.encode_to_vec().into());
      assert!(failure(&encoded).is_none());
      assert_eq!(encoded.code(), cause.code());
    }
  }

  #[test]
  fn conflicting_envelopes_and_duplicate_receipts_leave_progress_unknown() {
    let cause = tonic::Status::unavailable("readback failed");
    let encoded = status(
      &cause,
      InputActionFailure {
        progress: Some(Progress::NotDelivered(())),
      },
    );
    let mut envelope = RpcStatus::decode(encoded.details()).unwrap();
    envelope.code = tonic::Code::PermissionDenied as i32;
    let wrong_code = tonic::Status::with_details(cause.code(), cause.message(), envelope.encode_to_vec().into());
    assert!(failure(&wrong_code).is_none());
    envelope.code = cause.code() as i32;
    envelope.details.push(envelope.details[0].clone());
    let duplicate = tonic::Status::with_details(cause.code(), cause.message(), envelope.encode_to_vec().into());
    assert!(failure(&duplicate).is_none());
  }
}
