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
    (ApplicationIdKind::WmClass, ApplicationActivationMethod::EwmhActiveWindow),
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
