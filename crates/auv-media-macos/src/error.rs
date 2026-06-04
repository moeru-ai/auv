use std::fmt;

/// Failure modes for a now-playing read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaError {
  /// The host is not macOS, so the MediaRemote read is unavailable.
  Unsupported,
  /// The native MediaRemote read failed (framework missing, symbol gated,
  /// callback timeout, ...).
  Native {
    message: String,
    recovery_hint: String,
  },
}

impl MediaError {
  pub(crate) fn native(message: String, recovery_hint: Option<String>) -> Self {
    MediaError::Native {
      message,
      recovery_hint: recovery_hint.unwrap_or_else(|| "retry the now-playing read".to_string()),
    }
  }
}

impl fmt::Display for MediaError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      MediaError::Unsupported => {
        write!(f, "now-playing read is only available on macOS")
      }
      MediaError::Native {
        message,
        recovery_hint,
      } => write!(
        f,
        "macos now-playing read failed: {message}; recovery={recovery_hint}"
      ),
    }
  }
}

impl std::error::Error for MediaError {}
