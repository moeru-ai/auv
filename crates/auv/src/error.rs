//! Protocol-neutral failures shared by domain operation interfaces.

/// Protocol-neutral category for a transport client failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientErrorKind {
  /// Authentication or authorization failed.
  Unauthorized,
  /// The selected service could not be reached in time.
  Unavailable,
  /// The request is invalid.
  InvalidRequest,
  /// A selector or precondition has more than one valid resolution.
  Ambiguous,
  /// The selected resource does not exist.
  NotFound,
  /// Current remote state conflicts with the request.
  Conflict,
  /// The remote service does not implement the operation.
  Unsupported,
  /// The response or transport failed without a more specific category.
  Protocol,
}

/// Transport failure with a stable category and preserved source error.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{message}")]
pub struct ClientError {
  kind: ClientErrorKind,
  message: String,
  #[source]
  source: tonic::Status,
}

impl ClientError {
  /// Returns the stable failure category.
  pub fn kind(&self) -> ClientErrorKind {
    self.kind
  }

  pub(crate) fn from_status(operation: &'static str, status: tonic::Status) -> Self {
    let kind = match status.code() {
      tonic::Code::Unauthenticated | tonic::Code::PermissionDenied => ClientErrorKind::Unauthorized,
      tonic::Code::Unavailable | tonic::Code::DeadlineExceeded | tonic::Code::Cancelled => ClientErrorKind::Unavailable,
      tonic::Code::InvalidArgument => ClientErrorKind::InvalidRequest,
      tonic::Code::FailedPrecondition => ClientErrorKind::Ambiguous,
      tonic::Code::NotFound => ClientErrorKind::NotFound,
      tonic::Code::AlreadyExists | tonic::Code::Aborted => ClientErrorKind::Conflict,
      tonic::Code::Unimplemented => ClientErrorKind::Unsupported,
      _ => ClientErrorKind::Protocol,
    };
    Self {
      kind,
      message: format!("{operation} failed: {status}"),
      source: status,
    }
  }
}

#[cfg(test)]
mod tests {
  use std::error::Error as _;

  use super::*;

  #[test]
  fn transport_status_remains_in_the_error_source_chain() {
    let error = ClientError::from_status("ListDevices", tonic::Status::unavailable("offline"));
    assert_eq!(error.kind(), ClientErrorKind::Unavailable);
    assert!(error.source().is_some());
  }

  // Pins the transport-category half of the Runner-route failure
  // classification: invoke frontends turn each ClientErrorKind into a typed
  // failure code, so a status code silently changing category would change
  // the code every CLI and MCP caller sees.
  #[test]
  fn transport_status_codes_pin_the_transport_category_table() {
    let cases = [
      (tonic::Status::unauthenticated("denied"), ClientErrorKind::Unauthorized),
      (tonic::Status::permission_denied("denied"), ClientErrorKind::Unauthorized),
      (tonic::Status::unavailable("offline"), ClientErrorKind::Unavailable),
      (tonic::Status::deadline_exceeded("slow"), ClientErrorKind::Unavailable),
      (tonic::Status::cancelled("stopped"), ClientErrorKind::Unavailable),
      (tonic::Status::invalid_argument("bad"), ClientErrorKind::InvalidRequest),
      (tonic::Status::failed_precondition("drifted"), ClientErrorKind::Ambiguous),
      (tonic::Status::not_found("missing"), ClientErrorKind::NotFound),
      (tonic::Status::already_exists("duplicate"), ClientErrorKind::Conflict),
      (tonic::Status::aborted("raced"), ClientErrorKind::Conflict),
      (tonic::Status::unimplemented("unsupported"), ClientErrorKind::Unsupported),
      (tonic::Status::internal("broken"), ClientErrorKind::Protocol),
      (tonic::Status::unknown("uncertain"), ClientErrorKind::Protocol),
      (tonic::Status::data_loss("lost"), ClientErrorKind::Protocol),
    ];
    for (status, kind) in cases {
      assert_eq!(ClientError::from_status("Runner capability RPC", status).kind(), kind);
    }
  }
}
