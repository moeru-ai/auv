//! Application activation encoding and validation shared by Runner clients and
//! servers.

use auv_api_proto::auv::api::driver::v1 as proto;
use auv_driver::{
  ActivationWindowObservation, ApplicationActivationMethod, ApplicationActivationResult, ApplicationActivationVerification,
  ApplicationIdKind, WindowRef,
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
  #[error("ActivateApplication response contained a window observation with an empty window_id")]
  EmptyWindowObservationReference,
  #[error("ActivateApplication response contained a window observation without a reason")]
  EmptyWindowObservationEvidence,
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
  };
  let method = match result.method {
    ApplicationActivationMethod::AppleScriptActivate => proto::ApplicationActivationMethod::AppleScriptActivate,
    ApplicationActivationMethod::ForegroundWindow => proto::ApplicationActivationMethod::ForegroundWindow,
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
    window_observation: result.window_observation.map(|observation| proto::ActivationWindowObservation {
      observation: Some(match observation {
        ActivationWindowObservation::Present { reference } => {
          proto::activation_window_observation::Observation::Present(proto::WindowPresent {
            reference: Some(proto::WindowRef {
              window_id: reference.id,
            }),
          })
        }
        ActivationWindowObservation::Absent => proto::activation_window_observation::Observation::Absent(proto::WindowAbsent {}),
        ActivationWindowObservation::Unknown { reason } => {
          proto::activation_window_observation::Observation::Unknown(proto::ObservationUnavailable { reason })
        }
      }),
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
    Ok(proto::ApplicationIdKind::Unspecified) | Err(_) => return Err(DecodeError::UnknownApplicationIdKind),
  };
  let method = match proto::ApplicationActivationMethod::try_from(response.method) {
    Ok(proto::ApplicationActivationMethod::AppleScriptActivate) => ApplicationActivationMethod::AppleScriptActivate,
    Ok(proto::ApplicationActivationMethod::ForegroundWindow) => ApplicationActivationMethod::ForegroundWindow,
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
  // An unset observation stays `None`: the Runner's driver version did not
  // observe, which is an absent fact, never an absence claim.
  let window_observation = response.window_observation.map(|observation| decode_window_observation(observation)).transpose()?;
  Ok(ApplicationActivationResult {
    requested_application_id: response.requested_application_id,
    application_id_kind,
    method,
    activated_window,
    verification,
    window_observation,
  })
}

/// Decodes one window observation, failing closed on an empty oneof arm, an
/// empty window reference, or an empty unknown reason, so a partial
/// observation never reads as a fact.
fn decode_window_observation(observation: proto::ActivationWindowObservation) -> Result<ActivationWindowObservation, DecodeError> {
  use proto::activation_window_observation::Observation;
  match observation.observation {
    Some(Observation::Present(present)) => {
      let reference = present.reference.ok_or(DecodeError::EmptyWindowObservationReference)?;
      if reference.window_id.trim().is_empty() {
        return Err(DecodeError::EmptyWindowObservationReference);
      }
      Ok(ActivationWindowObservation::Present {
        reference: WindowRef {
          id: reference.window_id,
        },
      })
    }
    Some(Observation::Absent(_)) => Ok(ActivationWindowObservation::Absent),
    Some(Observation::Unknown(unknown)) => {
      if unknown.reason.trim().is_empty() {
        return Err(DecodeError::EmptyWindowObservationEvidence);
      }
      Ok(ActivationWindowObservation::Unknown {
        reason: unknown.reason,
      })
    }
    None => Err(DecodeError::EmptyWindowObservationEvidence),
  }
}

#[cfg(test)]
#[path = "application_test.rs"]
mod tests;
