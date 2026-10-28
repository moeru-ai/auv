//! Shared wire contract for capture admission failures in Runner and clients.
use prost::Message as _;

use crate::auv::api::driver::v1::{ClickPreconditionFailure, ClickPreconditionFailureReason};
use crate::google::rpc::Status as RpcStatus;

const TYPE_URL: &str = "type.googleapis.com/auv.api.driver.v1.ClickPreconditionFailure";

/// Wraps the typed refusal in the standard gRPC rich-error envelope.
pub fn status(capture_id: &str, reason: ClickPreconditionFailureReason, cause: &str) -> tonic::Status {
  encode_status(capture_id, reason, cause, None)
}

/// Retains the underlying check's gRPC category without changing the refusal.
pub fn status_with_cause(capture_id: &str, reason: ClickPreconditionFailureReason, cause: &tonic::Status) -> tonic::Status {
  encode_status(capture_id, reason, cause.message(), Some(cause.code()))
}

fn encode_status(capture_id: &str, reason: ClickPreconditionFailureReason, cause: &str, code: Option<tonic::Code>) -> tonic::Status {
  let message =
    format!("click capture precondition failed ({}): {cause}; take a new capture and select the target again", reason.as_str_name());
  let detail = prost_types::Any {
    type_url: TYPE_URL.into(),
    value: ClickPreconditionFailure {
      capture_id: capture_id.into(),
      reason: reason as i32,
      cause_code: code.filter(|code| *code != tonic::Code::Ok).map(|code| code as i32),
    }
    .encode_to_vec(),
  };
  let envelope = RpcStatus {
    code: tonic::Code::FailedPrecondition as i32,
    message: message.clone(),
    details: vec![detail],
  };
  tonic::Status::with_details(tonic::Code::FailedPrecondition, message, envelope.encode_to_vec().into())
}

/// Unrelated, malformed, and unknown failures retain their original status.
pub fn failure(status: &tonic::Status) -> Option<ClickPreconditionFailure> {
  if status.code() != tonic::Code::FailedPrecondition {
    return None;
  }
  let envelope = RpcStatus::decode(status.details()).ok()?;
  envelope.details.iter().filter(|detail| detail.type_url == TYPE_URL).find_map(|detail| {
    let failure = ClickPreconditionFailure::decode(detail.value.as_slice()).ok()?;
    let reason = ClickPreconditionFailureReason::try_from(failure.reason).ok()?;
    (reason != ClickPreconditionFailureReason::Unspecified).then_some(failure)
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  // https://github.com/moeru-ai/auv/pull/429#discussion_r4382693059
  #[test]
  fn refusal_uses_the_standard_status_and_any_envelope() {
    let status = status("cap", ClickPreconditionFailureReason::CheckUnavailable, "permission denied");
    let envelope = RpcStatus::decode(status.details()).unwrap();
    assert_eq!(envelope.code, 9);
    assert_eq!(envelope.message, status.message());
    assert_eq!(envelope.details[0].type_url, TYPE_URL);
    assert_eq!(failure(&status).unwrap().capture_id, "cap");
    assert_eq!(failure(&status).unwrap().cause_code, None);
    assert!(status.message().contains("permission denied"));
  }

  // https://github.com/moeru-ai/auv/pull/429#discussion_r4389303193
  #[test]
  fn original_check_code_survives_the_rich_error_envelope_pr_429() {
    let source = tonic::Status::permission_denied("screen recording permission revoked");
    let status = status_with_cause("cap", ClickPreconditionFailureReason::CheckUnavailable, &source);
    assert_eq!(status.code(), tonic::Code::FailedPrecondition);
    let detail = failure(&status).unwrap();
    assert_eq!(detail.cause_code, Some(tonic::Code::PermissionDenied as i32));
    assert_eq!(detail.reason, ClickPreconditionFailureReason::CheckUnavailable as i32);
    assert!(status.message().contains(source.message()));
  }

  #[test]
  fn unrelated_malformed_and_unknown_details_do_not_replace_the_status() {
    let envelopes = [
      RpcStatus {
        details: vec![prost_types::Any {
          type_url: "type.googleapis.com/other.Error".into(),
          value: vec![255],
        }],
        ..Default::default()
      },
      RpcStatus {
        details: vec![prost_types::Any {
          type_url: TYPE_URL.into(),
          value: vec![255],
        }],
        ..Default::default()
      },
      RpcStatus {
        details: vec![prost_types::Any {
          type_url: TYPE_URL.into(),
          value: ClickPreconditionFailure {
            capture_id: "cap".into(),
            reason: 99,
            cause_code: None,
          }
          .encode_to_vec(),
        }],
        ..Default::default()
      },
    ];
    for envelope in envelopes {
      let status = tonic::Status::with_details(tonic::Code::FailedPrecondition, "original", envelope.encode_to_vec().into());
      assert!(failure(&status).is_none());
    }
    assert!(failure(&tonic::Status::with_details(tonic::Code::FailedPrecondition, "original", vec![255].into())).is_none());
  }
}
