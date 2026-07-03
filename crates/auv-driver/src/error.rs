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
      Self::InvalidInput { message } | Self::Backend { message } => f.write_str(message),
    }
  }
}

impl Error for DriverError {}
