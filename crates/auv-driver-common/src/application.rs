use serde::{Deserialize, Serialize};

/// Evidence returned after requesting foreground activation for one app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationActivationResult {
  /// Bundle identifier supplied to the activation request.
  pub requested_bundle_id: String,
  /// Evidence from the post-activation foreground observation.
  pub verification: ApplicationActivationVerification,
}

/// What a post-activation platform observation could establish.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ApplicationActivationVerification {
  /// The requested application was observed as the foreground application.
  VerifiedForeground {
    /// Bundle identifier returned by the foreground observation.
    observed_bundle_id: String,
  },
  /// Activation was delivered, but another application was observed in front.
  #[serde(rename = "activation_only_foreground_mismatch")]
  ForegroundMismatch {
    /// Bundle identifier returned by the foreground observation.
    observed_bundle_id: String,
  },
  /// Activation was delivered, but no foreground observation was available.
  #[serde(rename = "activation_only_verification_unavailable")]
  Unavailable {
    /// Why the platform could not provide foreground evidence.
    reason: String,
  },
}

impl ApplicationActivationVerification {
  /// Returns the canonical status used by human and structured output.
  pub fn status(&self) -> &'static str {
    match self {
      Self::VerifiedForeground { .. } => "verified_foreground",
      Self::ForegroundMismatch { .. } => "activation_only_foreground_mismatch",
      Self::Unavailable { .. } => "activation_only_verification_unavailable",
    }
  }

  /// Returns the observed foreground bundle identifier when one was available.
  pub fn observed_bundle_id(&self) -> Option<&str> {
    match self {
      Self::VerifiedForeground { observed_bundle_id } | Self::ForegroundMismatch { observed_bundle_id } => Some(observed_bundle_id),
      Self::Unavailable { .. } => None,
    }
  }
}

/// Evidence returned after requesting foreground activation for one process.
///
/// Same shape as [`ApplicationActivationResult`], but keyed by process name
/// rather than a macOS bundle identifier for platforms with no app-bundle
/// concept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessActivationResult {
  /// Process name supplied to the activation request.
  pub requested_process: String,
  /// Evidence from the post-activation foreground observation.
  pub verification: ProcessActivationVerification,
}

/// What a post-activation platform observation could establish, keyed by
/// process name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProcessActivationVerification {
  /// The requested process was observed owning the foreground window.
  VerifiedForeground {
    /// Process name returned by the foreground observation.
    observed_process: String,
  },
  /// Activation was delivered, but another process was observed in front.
  #[serde(rename = "activation_only_foreground_mismatch")]
  ForegroundMismatch {
    /// Process name returned by the foreground observation.
    observed_process: String,
  },
  /// Activation was delivered, but no foreground observation was available.
  #[serde(rename = "activation_only_verification_unavailable")]
  Unavailable {
    /// Why the platform could not provide foreground evidence.
    reason: String,
  },
}

impl ProcessActivationVerification {
  /// Returns the canonical status used by human and structured output.
  pub fn status(&self) -> &'static str {
    match self {
      Self::VerifiedForeground { .. } => "verified_foreground",
      Self::ForegroundMismatch { .. } => "activation_only_foreground_mismatch",
      Self::Unavailable { .. } => "activation_only_verification_unavailable",
    }
  }

  /// Returns the observed foreground process name when one was available.
  pub fn observed_process(&self) -> Option<&str> {
    match self {
      Self::VerifiedForeground { observed_process } | Self::ForegroundMismatch { observed_process } => Some(observed_process),
      Self::Unavailable { .. } => None,
    }
  }
}

#[cfg(test)]
#[path = "application_test.rs"]
mod tests;
