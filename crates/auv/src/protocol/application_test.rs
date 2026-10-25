use super::*;

fn result(verification: ApplicationActivationVerification) -> ApplicationActivationResult {
  ApplicationActivationResult {
    requested_application_id: "notepad.exe".to_string(),
    application_id_kind: ApplicationIdKind::ProcessName,
    method: ApplicationActivationMethod::ForegroundWindow,
    activated_window: Some(WindowRef {
      id: "0x1234".to_string(),
    }),
    verification,
    window_observation: None,
  }
}

#[test]
fn roundtrip_preserves_each_verification_variant() {
  for verification in [
    ApplicationActivationVerification::VerifiedForeground {
      observed_application_id: "notepad.exe".to_string(),
    },
    ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id: "explorer.exe".to_string(),
    },
    ApplicationActivationVerification::Unavailable {
      reason: "foreground window observation failed".to_string(),
    },
  ] {
    let original = result(verification);
    assert_eq!(decode(encode(original.clone())).expect("round trip"), original);
  }
}

#[test]
fn roundtrip_preserves_every_kind_and_method_and_an_absent_window() {
  for (kind, method) in [
    (ApplicationIdKind::BundleId, ApplicationActivationMethod::AppleScriptActivate),
    (ApplicationIdKind::ProcessName, ApplicationActivationMethod::ForegroundWindow),
    (ApplicationIdKind::DesktopAppId, ApplicationActivationMethod::ForegroundWindow),
  ] {
    let original = ApplicationActivationResult {
      application_id_kind: kind,
      method,
      activated_window: None,
      ..result(ApplicationActivationVerification::VerifiedForeground {
        observed_application_id: "notepad.exe".to_string(),
      })
    };
    assert_eq!(decode(encode(original.clone())).expect("round trip"), original);
  }
}

#[test]
fn decode_rejects_missing_or_empty_evidence() {
  let valid = encode(result(ApplicationActivationVerification::VerifiedForeground {
    observed_application_id: "notepad.exe".to_string(),
  }));

  let missing_verification = proto::ActivateApplicationResponse {
    verification: None,
    ..valid.clone()
  };
  assert_eq!(decode(missing_verification), Err(DecodeError::MissingVerification));

  let empty_reason = proto::ActivateApplicationResponse {
    verification: Some(proto::ApplicationActivationVerification {
      verification: Some(proto::application_activation_verification::Verification::Unavailable(proto::VerificationUnavailable::default())),
    }),
    ..valid.clone()
  };
  assert_eq!(decode(empty_reason), Err(DecodeError::EmptyVerificationEvidence));

  let blank_request = proto::ActivateApplicationResponse {
    requested_application_id: " ".to_string(),
    ..valid.clone()
  };
  assert_eq!(decode(blank_request), Err(DecodeError::MissingRequestedApplicationId));

  let unspecified_kind = proto::ActivateApplicationResponse {
    application_id_kind: proto::ApplicationIdKind::Unspecified as i32,
    ..valid.clone()
  };
  assert_eq!(decode(unspecified_kind), Err(DecodeError::UnknownApplicationIdKind));

  let unknown_method = proto::ActivateApplicationResponse {
    method: 99,
    ..valid.clone()
  };
  assert_eq!(decode(unknown_method), Err(DecodeError::UnknownMethod));

  let empty_window = proto::ActivateApplicationResponse {
    activated_window: Some(proto::WindowRef::default()),
    ..valid
  };
  assert_eq!(decode(empty_window), Err(DecodeError::EmptyActivatedWindow));
}

// ROOT CAUSE (issue #324, wire half):
//
// The ActivateApplication wire shape carried no window facts, so a Runner
// route caller learned only that some app was frontmost and had to guess
// whether a window-targeted follow-up could run.
//
// The response now carries an optional window observation. An unset field
// decodes to `None` (this Runner did not observe — an absent fact, never an
// absence claim), and every present observation must carry complete
// evidence: a non-empty window reference or a non-empty unknown reason.
#[test]
fn roundtrip_preserves_each_window_observation_and_an_unset_one() {
  for observation in [
    Some(ActivationWindowObservation::Present {
      reference: WindowRef {
        id: "0x1234".to_string(),
      },
    }),
    Some(ActivationWindowObservation::Absent),
    Some(ActivationWindowObservation::Unknown {
      reason: "window observation failed".to_string(),
    }),
    None,
  ] {
    let original = ApplicationActivationResult {
      window_observation: observation,
      ..result(ApplicationActivationVerification::VerifiedForeground {
        observed_application_id: "notepad.exe".to_string(),
      })
    };
    assert_eq!(decode(encode(original.clone())).expect("round trip"), original);
  }
}

#[test]
fn decode_rejects_partial_window_observations_and_accepts_unset_ones() {
  use proto::activation_window_observation::Observation;

  let valid = encode(result(ApplicationActivationVerification::VerifiedForeground {
    observed_application_id: "notepad.exe".to_string(),
  }));

  // An old Runner's response without the field decodes with no fact.
  let unset = proto::ActivateApplicationResponse {
    window_observation: None,
    ..valid.clone()
  };
  assert!(decode(unset).expect("unset observation decodes").window_observation.is_none());

  let empty_oneof = proto::ActivateApplicationResponse {
    window_observation: Some(proto::ActivationWindowObservation { observation: None }),
    ..valid.clone()
  };
  assert_eq!(decode(empty_oneof), Err(DecodeError::EmptyWindowObservationEvidence));

  let empty_reference = proto::ActivateApplicationResponse {
    window_observation: Some(proto::ActivationWindowObservation {
      observation: Some(Observation::Present(proto::WindowPresent {
        reference: Some(proto::WindowRef::default()),
      })),
    }),
    ..valid.clone()
  };
  assert_eq!(decode(empty_reference), Err(DecodeError::EmptyWindowObservationReference));

  let missing_reference = proto::ActivateApplicationResponse {
    window_observation: Some(proto::ActivationWindowObservation {
      observation: Some(Observation::Present(proto::WindowPresent { reference: None })),
    }),
    ..valid.clone()
  };
  assert_eq!(decode(missing_reference), Err(DecodeError::EmptyWindowObservationReference));

  let empty_unknown_reason = proto::ActivateApplicationResponse {
    window_observation: Some(proto::ActivationWindowObservation {
      observation: Some(Observation::Unknown(proto::ObservationUnavailable::default())),
    }),
    ..valid
  };
  assert_eq!(decode(empty_unknown_reason), Err(DecodeError::EmptyWindowObservationEvidence));
}
