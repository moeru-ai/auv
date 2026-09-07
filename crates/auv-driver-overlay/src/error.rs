use std::fmt;

pub type OverlayResult<T> = Result<T, OverlayError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayError {
  /// Host appearance configuration was rejected before native rendering.
  InvalidTheme {
    message: String,
  },
  Unavailable {
    reason: String,
  },
  Backend {
    message: String,
  },
}

impl OverlayError {
  #[cfg(any(
    all(target_os = "macos", feature = "macos"),
    all(target_os = "windows", feature = "windows")
  ))]
  pub(crate) fn backend(message: String) -> Self {
    Self::Backend { message }
  }
}

impl fmt::Display for OverlayError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::InvalidTheme { message } => write!(formatter, "invalid overlay theme: {message}"),
      Self::Unavailable { reason } => write!(formatter, "overlay unavailable: {reason}"),
      Self::Backend { message } => write!(formatter, "overlay backend failed: {message}"),
    }
  }
}

impl std::error::Error for OverlayError {}
