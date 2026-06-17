//! Internal helpers for constructing shared [`DriverError`] variants.

use auv_driver::error::DriverError;

pub(crate) fn backend(message: impl std::fmt::Display) -> DriverError {
  DriverError::Backend {
    message: message.to_string(),
  }
}

pub(crate) fn invalid_input(message: impl std::fmt::Display) -> DriverError {
  DriverError::InvalidInput {
    message: message.to_string(),
  }
}

pub(crate) fn not_found(target: impl std::fmt::Display) -> DriverError {
  DriverError::NotFound {
    target: target.to_string(),
  }
}
