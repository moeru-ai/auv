use std::error::Error;
use std::fmt;

pub type DriverResult<T> = Result<T, DriverError>;

#[derive(Debug)]
pub enum DriverError {
  Unsupported {
    operation: &'static str,
  },
  NotFound {
    target: String,
  },
  /// A previously admitted input recipient lost its identity or delivery preconditions.
  TargetLost {
    target: String,
    message: String,
  },
  PermissionDenied {
    permission: &'static str,
    /// Raw backend detail when the permission failure came from a native
    /// response. Locally detected gates may leave this empty.
    message: Option<String>,
    recovery: Option<String>,
  },
  InvalidInput {
    message: String,
  },
  /// A previously resolved UI reference (e.g. an AX path) no longer
  /// resolves against the live UI — the tree shifted since it was observed.
  /// Distinct from `NotFound` (which means a target was never located) and from
  /// `InvalidInput` (which means the caller supplied a malformed request).
  StaleUiReference {
    message: String,
    recovery: Option<String>,
  },
  /// A node resolved at the requested location, but its role differs from the
  /// expected role — a specific, recoverable form of tree drift that callers
  /// may want to distinguish from a fully unresolved path.
  RoleMismatch {
    message: String,
    recovery: Option<String>,
  },
  Backend {
    message: String,
  },
  /// One input operation failed with a known native delivery boundary.
  /// The original error category remains in `cause`; never infer zero input
  /// from errors that do not carry this receipt.
  InputAction {
    cause: Box<DriverError>,
    progress: crate::InputActionProgress,
  },
}

impl DriverError {
  pub fn unsupported(operation: &'static str) -> Self {
    Self::Unsupported { operation }
  }
}

impl fmt::Display for DriverError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Unsupported { operation } => write!(f, "{operation} is not supported by this driver"),
      Self::NotFound { target } => write!(f, "{target} was not found"),
      Self::TargetLost { target, message } => write!(f, "input target {target} was lost: {message}; inspect state before retrying"),
      Self::PermissionDenied {
        permission,
        message,
        recovery,
      } => {
        write!(f, "{permission} permission was denied")?;
        match (message, recovery) {
          (Some(message), Some(recovery)) => write!(f, ": {message}; recovery: {recovery}")?,
          (Some(message), None) => write!(f, ": {message}")?,
          (None, Some(recovery)) => write!(f, ": {recovery}")?,
          (None, None) => {}
        }
        Ok(())
      }
      Self::StaleUiReference { message, recovery } | Self::RoleMismatch { message, recovery } => {
        f.write_str(message)?;
        if let Some(recovery) = recovery {
          write!(f, ": {recovery}")?;
        }
        Ok(())
      }
      Self::InvalidInput { message } | Self::Backend { message } => f.write_str(message),
      Self::InputAction { cause, progress } => {
        match progress {
          crate::InputActionProgress::NotDelivered => {}
          crate::InputActionProgress::DeliveryUncertain { .. } => {
            f.write_str("input delivery is uncertain; inspect state before retrying: ")?;
          }
          crate::InputActionProgress::Submitted { .. } => {
            f.write_str("input was submitted but is unverified; inspect state before retrying: ")?;
          }
        }
        cause.fmt(f)
      }
    }
  }
}

impl Error for DriverError {
  fn source(&self) -> Option<&(dyn Error + 'static)> {
    match self {
      Self::InputAction { cause, .. } => Some(cause.as_ref()),
      _ => None,
    }
  }
}
