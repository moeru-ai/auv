//! Shared translation from server-domain failures to gRPC status codes.

use tonic::Status;

pub(crate) fn map_control_error(error: crate::daemon::DaemonError) -> Status {
  use crate::daemon::DaemonError;
  match error {
    DaemonError::Identity(_) => Status::internal(error.to_string()),
    DaemonError::InvalidArgument(_) | DaemonError::UnknownDevice(_) => Status::invalid_argument(error.to_string()),
    DaemonError::UnknownRun(_) | DaemonError::UnknownRunner(_) => Status::not_found(error.to_string()),
    DaemonError::RunnerProviderUnavailable(_) => Status::unimplemented(error.to_string()),
    DaemonError::RunnerOperation(_) => Status::unavailable(error.to_string()),
  }
}
