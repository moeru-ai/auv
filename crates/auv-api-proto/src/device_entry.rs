//! Wire encoding of Device entry failures.
//!
//! A failed session request (`ListUserSessions`, `GetUserSession`,
//! `EnsureUserSessionUnlocked`, `EnsureUserSessionLocked`) returns a gRPC
//! status. Its code follows the reason, and `google.rpc.Status.details`
//! carries one `DeviceEntryError` packed in `Any`, so gRPC tooling sees a
//! failure while SDKs still read the typed reason. Both the daemon and the
//! clients use this module, so the mapping exists in one place.

use prost::Message as _;

use crate::auv::api::daemon::v1::{DeviceEntryError, DeviceEntryErrorReason};
use crate::google::rpc::Status as RpcStatus;

const TYPE_URL: &str = "type.googleapis.com/auv.api.daemon.v1.DeviceEntryError";

/// The gRPC code that best describes a reason to generic tooling.
pub fn code(reason: DeviceEntryErrorReason) -> tonic::Code {
  use DeviceEntryErrorReason as Reason;

  match reason {
    Reason::Unauthorized | Reason::Disabled => tonic::Code::PermissionDenied,
    Reason::AmbiguousUser => tonic::Code::InvalidArgument,
    // The selected login instance changed; list sessions again and retry.
    Reason::StaleSession => tonic::Code::Aborted,
    Reason::ServiceUnavailable | Reason::AuditUnavailable => tonic::Code::Unavailable,
    // The OS may or may not have applied the change.
    Reason::OutcomeUnverified => tonic::Code::Unknown,
    Reason::Unenrolled
    | Reason::Suspended
    | Reason::OccupiedDesktop
    | Reason::UnsupportedOsState
    | Reason::CredentialRejected
    | Reason::HostIncompatible
    | Reason::Unspecified => tonic::Code::FailedPrecondition,
  }
}

/// Builds the failing status for one reason. The message names the reason so
/// that clients without the typed decoder still see it.
pub fn status(reason: DeviceEntryErrorReason) -> tonic::Status {
  let code = code(reason);
  let message = format!("Device entry failed: {}", reason.as_str_name());
  let detail = prost_types::Any {
    type_url: TYPE_URL.to_string(),
    value: DeviceEntryError {
      reason: reason as i32,
    }
    .encode_to_vec(),
  };
  let envelope = RpcStatus {
    code: code as i32,
    message: message.clone(),
    details: vec![detail],
  };
  tonic::Status::with_details(code, message, envelope.encode_to_vec().into())
}

/// Reads the typed reason from a failing status, or `None` when the status
/// carries no `DeviceEntryError` (a transport or argument failure, or an
/// older daemon).
pub fn reason(status: &tonic::Status) -> Option<DeviceEntryErrorReason> {
  if status.details().is_empty() {
    return None;
  }
  let envelope = RpcStatus::decode(status.details()).ok()?;
  envelope
    .details
    .iter()
    .find(|detail| detail.type_url == TYPE_URL)
    .and_then(|detail| DeviceEntryError::decode(detail.value.as_slice()).ok())
    .and_then(|error| DeviceEntryErrorReason::try_from(error.reason).ok())
    .filter(|reason| *reason != DeviceEntryErrorReason::Unspecified)
}

#[cfg(test)]
#[path = "device_entry_test.rs"]
mod tests;
