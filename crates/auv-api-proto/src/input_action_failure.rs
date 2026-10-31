//! Shared rich-error envelope for native input failures and keyboard progress.

use prost::Message as _;

use crate::auv::api::driver::v1::{InputActionFailure, InputFailureReason, KeyboardInputProgress};
use crate::google::rpc::Status as RpcStatus;

const TYPE_URL: &str = "type.googleapis.com/auv.api.driver.v1.InputActionFailure";
const KEYBOARD_TYPE_URL: &str = "type.googleapis.com/auv.api.driver.v1.KeyboardInputProgress";

/// Packs a domain-validated failure while retaining its gRPC category/message.
pub fn status(cause: &tonic::Status, failure: InputActionFailure) -> tonic::Status {
  if !valid_failure(cause.code(), &failure) {
    return tonic::Status::internal("input action failure requires a consistent failed status and explicit progress or reason");
  }
  with_detail(cause, TYPE_URL, failure)
}

/// Unknown, ambiguous, malformed, or unrelated details leave progress unknown.
/// Callers must also validate the domain action before treating it as evidence.
pub fn failure(status: &tonic::Status) -> Option<InputActionFailure> {
  let failure = detail::<InputActionFailure>(status, TYPE_URL)?;
  valid_failure(status.code(), &failure).then_some(failure)
}

/// Adds batch progress without discarding a native receipt in the same status.
pub fn keyboard_status(cause: &tonic::Status, progress: KeyboardInputProgress) -> tonic::Status {
  if cause.code() == tonic::Code::Ok || progress.action_index.is_none() {
    return tonic::Status::internal("keyboard failure requires a failed status and an explicit action index");
  }
  with_detail(cause, KEYBOARD_TYPE_URL, progress)
}

/// Reads standard rich-error details.
/// NOTICE: Legacy raw details leave progress unknown; upgrade Runner and SDK together.
pub fn keyboard_progress(status: &tonic::Status) -> Option<KeyboardInputProgress> {
  let progress = detail::<KeyboardInputProgress>(status, KEYBOARD_TYPE_URL)?;
  progress.action_index.is_some().then_some(progress)
}

// The envelope is shared so composing batch and native failures cannot replace
// one receipt with another. Conflicting framing or duplicate types are rejected.
fn with_detail(cause: &tonic::Status, type_url: &str, value: impl prost::Message) -> tonic::Status {
  let mut envelope = if cause.details().is_empty() {
    RpcStatus {
      code: cause.code() as i32,
      message: cause.message().into(),
      details: vec![],
    }
  } else {
    match RpcStatus::decode(cause.details()) {
      Ok(envelope) if envelope.code == cause.code() as i32 && envelope.message == cause.message() => envelope,
      _ => return tonic::Status::internal("input failure has invalid rich-error framing"),
    }
  };
  if envelope.details.iter().any(|detail| detail.type_url == type_url) {
    return tonic::Status::internal("input failure has duplicate rich-error details");
  }
  envelope.details.push(prost_types::Any {
    type_url: type_url.into(),
    value: value.encode_to_vec(),
  });
  tonic::Status::with_details(cause.code(), cause.message(), envelope.encode_to_vec().into())
}

fn detail<T: prost::Message + Default>(status: &tonic::Status, type_url: &str) -> Option<T> {
  if status.code() == tonic::Code::Ok {
    return None;
  }
  let envelope = RpcStatus::decode(status.details()).ok()?;
  if envelope.code != status.code() as i32 || envelope.message != status.message() {
    return None;
  }
  let mut matching = envelope.details.iter().filter(|detail| detail.type_url == type_url);
  let detail = matching.next()?;
  if matching.next().is_some() {
    return None;
  }
  T::decode(detail.value.as_slice()).ok()
}

fn valid_failure(code: tonic::Code, failure: &InputActionFailure) -> bool {
  let target_lost = failure.reason == InputFailureReason::TargetLost as i32;
  code != tonic::Code::Ok && (!target_lost || code == tonic::Code::FailedPrecondition) && (failure.progress.is_some() || target_lost)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::auv::api::driver::v1::input_action_failure::Progress;

  #[test]
  fn target_loss_can_be_known_while_delivery_remains_unknown() {
    let cause = tonic::Status::failed_precondition("recipient disappeared");
    let encoded = status(
      &cause,
      InputActionFailure {
        progress: None,
        reason: InputFailureReason::TargetLost as i32,
      },
    );
    let receipt = failure(&encoded).unwrap();
    assert_eq!(receipt.reason, InputFailureReason::TargetLost as i32);
    assert!(receipt.progress.is_none());
    assert!(keyboard_progress(&encoded).is_none());
    assert_eq!(status(&tonic::Status::permission_denied("denied"), receipt).code(), tonic::Code::Internal);
  }

  #[test]
  fn batch_and_native_receipts_coexist_without_changing_the_cause() {
    let cause = tonic::Status::failed_precondition("recipient changed");
    let native = status(
      &cause,
      InputActionFailure {
        progress: Some(Progress::NotDelivered(())),
        reason: InputFailureReason::TargetLost as i32,
      },
    );
    let batch = KeyboardInputProgress {
      action_index: Some(2),
      completed_presses: 1,
      completed: vec![],
    };
    let encoded = keyboard_status(&native, batch.clone());
    assert_eq!(encoded.code(), cause.code());
    assert_eq!(encoded.message(), cause.message());
    assert_eq!(keyboard_progress(&encoded), Some(batch.clone()));
    assert!(matches!(failure(&encoded).unwrap().progress, Some(Progress::NotDelivered(()))));
    assert_eq!(keyboard_status(&encoded, batch).code(), tonic::Code::Internal);
  }

  #[test]
  fn legacy_raw_batch_metadata_and_missing_indices_remain_unknown() {
    let cause = tonic::Status::unavailable("worker stopped");
    let raw = KeyboardInputProgress {
      action_index: Some(0),
      ..Default::default()
    };
    let status = tonic::Status::with_details(cause.code(), cause.message(), raw.encode_to_vec().into());
    assert!(keyboard_progress(&status).is_none());
    assert!(failure(&status).is_none());
    assert_eq!(keyboard_status(&cause, KeyboardInputProgress::default()).code(), tonic::Code::Internal);
  }

  #[test]
  fn an_explicit_zero_delivery_receipt_keeps_the_original_category() {
    let cause = tonic::Status::permission_denied("UIA permission denied");
    let encoded = status(
      &cause,
      InputActionFailure {
        progress: Some(Progress::NotDelivered(())),
        reason: InputFailureReason::Unspecified as i32,
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
        reason: InputFailureReason::Unspecified as i32,
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
