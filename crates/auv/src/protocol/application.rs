//! Application activation encoding and validation shared by Runner clients and
//! servers.

use auv_api_proto::auv::api::driver::v1 as proto;
use auv_driver::{
  ApplicationActivationMethod, ApplicationActivationResult, ApplicationActivationVerification, ApplicationIdKind, WindowRef,
};

/// A wire activation response that does not carry complete typed evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
  #[error("ActivateApplication response omitted requested_application_id")]
  MissingRequestedApplicationId,
  #[error("ActivateApplication response has an unspecified or unknown application_id_kind")]
  UnknownApplicationIdKind,
  #[error("ActivateApplication response has an unspecified or unknown method")]
  UnknownMethod,
  #[error("ActivateApplication response contained an activated_window with an empty window_id")]
  EmptyActivatedWindow,
  #[error("ActivateApplication response omitted verification")]
  MissingVerification,
  #[error("ActivateApplication response contained empty verification evidence")]
  EmptyVerificationEvidence,
}

/// Encodes a driver activation result without validation.
pub fn encode(result: ApplicationActivationResult) -> proto::ActivateApplicationResponse {
  use proto::application_activation_verification::Verification;

  let verification = match result.verification {
    ApplicationActivationVerification::VerifiedForeground {
      observed_application_id,
    } => Verification::VerifiedForeground(proto::VerifiedForeground {
      observed_application_id,
    }),
    ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id,
    } => Verification::ForegroundMismatch(proto::ForegroundMismatch {
      observed_application_id,
    }),
    ApplicationActivationVerification::Unavailable { reason } => Verification::Unavailable(proto::VerificationUnavailable { reason }),
  };
  let application_id_kind = match result.application_id_kind {
    ApplicationIdKind::BundleId => proto::ApplicationIdKind::BundleId,
    ApplicationIdKind::ProcessName => proto::ApplicationIdKind::ProcessName,
    ApplicationIdKind::DesktopAppId => proto::ApplicationIdKind::DesktopAppId,
    ApplicationIdKind::WmClass => proto::ApplicationIdKind::WmClass,
  };
  let method = match result.method {
    ApplicationActivationMethod::AppleScriptActivate => proto::ApplicationActivationMethod::AppleScriptActivate,
    ApplicationActivationMethod::ForegroundWindow => proto::ApplicationActivationMethod::ForegroundWindow,
    ApplicationActivationMethod::EwmhActiveWindow => proto::ApplicationActivationMethod::EwmhActiveWindow,
  };
  proto::ActivateApplicationResponse {
    requested_application_id: result.requested_application_id,
    application_id_kind: application_id_kind as i32,
    method: method as i32,
    activated_window: result.activated_window.map(|window| proto::WindowRef {
      window_id: window.id,
    }),
    verification: Some(proto::ApplicationActivationVerification {
      verification: Some(verification),
    }),
  }
}

/// Decodes a response into typed evidence, failing closed on any missing or
/// unknown field so callers never mistake partial evidence for verification.
pub fn decode(response: proto::ActivateApplicationResponse) -> Result<ApplicationActivationResult, DecodeError> {
  use proto::application_activation_verification::Verification;

  if response.requested_application_id.trim().is_empty() {
    return Err(DecodeError::MissingRequestedApplicationId);
  }
  let application_id_kind = match proto::ApplicationIdKind::try_from(response.application_id_kind) {
    Ok(proto::ApplicationIdKind::BundleId) => ApplicationIdKind::BundleId,
    Ok(proto::ApplicationIdKind::ProcessName) => ApplicationIdKind::ProcessName,
    Ok(proto::ApplicationIdKind::DesktopAppId) => ApplicationIdKind::DesktopAppId,
    Ok(proto::ApplicationIdKind::WmClass) => ApplicationIdKind::WmClass,
    Ok(proto::ApplicationIdKind::Unspecified) | Err(_) => return Err(DecodeError::UnknownApplicationIdKind),
  };
  let method = match proto::ApplicationActivationMethod::try_from(response.method) {
    Ok(proto::ApplicationActivationMethod::AppleScriptActivate) => ApplicationActivationMethod::AppleScriptActivate,
    Ok(proto::ApplicationActivationMethod::ForegroundWindow) => ApplicationActivationMethod::ForegroundWindow,
    Ok(proto::ApplicationActivationMethod::EwmhActiveWindow) => ApplicationActivationMethod::EwmhActiveWindow,
    Ok(proto::ApplicationActivationMethod::Unspecified) | Err(_) => return Err(DecodeError::UnknownMethod),
  };
  let activated_window = match response.activated_window {
    Some(window) if window.window_id.trim().is_empty() => return Err(DecodeError::EmptyActivatedWindow),
    Some(window) => Some(WindowRef {
      id: window.window_id,
    }),
    None => None,
  };
  let verification = response.verification.and_then(|verification| verification.verification).ok_or(DecodeError::MissingVerification)?;
  let verification = match verification {
    Verification::VerifiedForeground(value) if !value.observed_application_id.trim().is_empty() => {
      ApplicationActivationVerification::VerifiedForeground {
        observed_application_id: value.observed_application_id,
      }
    }
    Verification::ForegroundMismatch(value) if !value.observed_application_id.trim().is_empty() => {
      ApplicationActivationVerification::ForegroundMismatch {
        observed_application_id: value.observed_application_id,
      }
    }
    Verification::Unavailable(value) if !value.reason.trim().is_empty() => ApplicationActivationVerification::Unavailable {
      reason: value.reason,
    },
    _ => return Err(DecodeError::EmptyVerificationEvidence),
  };
  Ok(ApplicationActivationResult {
    requested_application_id: response.requested_application_id,
    application_id_kind,
    method,
    activated_window,
    verification,
  })
}

#[cfg(test)]
#[path = "application_test.rs"]
mod tests;
