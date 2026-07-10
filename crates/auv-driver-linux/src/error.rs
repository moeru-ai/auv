use auv_driver_common::error::DriverError;

pub(crate) fn backend(message: impl Into<String>) -> DriverError {
  DriverError::Backend {
    message: message.into(),
  }
}

pub(crate) fn invalid_input(message: impl Into<String>) -> DriverError {
  DriverError::InvalidInput {
    message: message.into(),
  }
}

pub(crate) fn not_found(target: impl Into<String>) -> DriverError {
  DriverError::NotFound {
    target: target.into(),
  }
}
