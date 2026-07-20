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
  PermissionDenied {
    permission: &'static str,
    recovery: Option<String>,
  },
  InvalidInput {
    message: String,
  },
  /// A recorded observation (e.g. an AX path or captured tree) no longer
  /// resolves against the live UI — the tree shifted since it was observed.
  /// Distinct from `NotFound` (which means a target was never located) and from
  /// `InvalidInput` (which means the caller supplied a malformed request).
  StaleObservation {
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
      Self::PermissionDenied {
        permission,
        recovery,
      } => {
        write!(f, "{permission} permission was denied")?;
        if let Some(recovery) = recovery {
          write!(f, ": {recovery}")?;
        }
        Ok(())
      }
      Self::StaleObservation { message, recovery } | Self::RoleMismatch { message, recovery } => {
        f.write_str(message)?;
        if let Some(recovery) = recovery {
          write!(f, ": {recovery}")?;
        }
        Ok(())
      }
      Self::InvalidInput { message } | Self::Backend { message } => f.write_str(message),
    }
  }
}

impl Error for DriverError {}
