use super::*;

#[test]
fn verification_serializes_the_same_canonical_status_reported_to_humans() {
  for verification in [
    ApplicationActivationVerification::VerifiedForeground {
      observed_application_id: "com.example.Editor".to_string(),
    },
    ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id: "com.example.Other".to_string(),
    },
    ApplicationActivationVerification::Unavailable {
      reason: "observation unavailable".to_string(),
    },
  ] {
    let serialized = serde_json::to_value(&verification).expect("activation verification should serialize");
    assert_eq!(serialized["status"], verification.status());
  }
}

// ROOT CAUSE:
//
// If the activation result changed its serialized status strings while the
// bundle-id and process-name results merged, invoke JSON consumers that branch
// on `verification.status` would silently stop matching.
//
// Before the merge, each platform had its own result type.
// The merge keeps the status strings and adds the id kind, method, and window.
#[test]
fn activation_result_round_trips_with_stable_status_strings_and_new_fields() {
  let result = ApplicationActivationResult {
    requested_application_id: "notepad.exe".to_string(),
    application_id_kind: ApplicationIdKind::ProcessName,
    method: ApplicationActivationMethod::ForegroundWindow,
    activated_window: Some(WindowRef {
      id: "0x1234".to_string(),
    }),
    verification: ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id: "explorer.exe".to_string(),
    },
    window_observation: None,
  };

  let serialized = serde_json::to_value(&result).expect("activation result should serialize");
  assert_eq!(
    serialized,
    serde_json::json!({
      "requested_application_id": "notepad.exe",
      "application_id_kind": "process_name",
      "method": "foreground_window",
      "activated_window": { "id": "0x1234" },
      "verification": {
        "status": "activation_only_foreground_mismatch",
        "observed_application_id": "explorer.exe",
      },
      "window_observation": null,
    })
  );
  let decoded: ApplicationActivationResult = serde_json::from_value(serialized).expect("activation result should deserialize");
  assert_eq!(decoded, result);

  for (verification, status) in [
    (
      ApplicationActivationVerification::VerifiedForeground {
        observed_application_id: "com.example.Editor".to_string(),
      },
      "verified_foreground",
    ),
    (
      ApplicationActivationVerification::Unavailable {
        reason: "observation unavailable".to_string(),
      },
      "activation_only_verification_unavailable",
    ),
  ] {
    assert_eq!(serde_json::to_value(&verification).expect("serialize")["status"], status);
  }
}

#[test]
fn id_kind_and_method_names_match_their_serialized_form() {
  for kind in [
    ApplicationIdKind::BundleId,
    ApplicationIdKind::ProcessName,
    ApplicationIdKind::DesktopAppId,
  ] {
    assert_eq!(serde_json::to_value(kind).expect("serialize"), kind.as_str());
  }
  for method in [
    ApplicationActivationMethod::AppleScriptActivate,
    ApplicationActivationMethod::ForegroundWindow,
  ] {
    assert_eq!(serde_json::to_value(method).expect("serialize"), method.as_str());
  }
}

#[test]
fn wayland_application_id_kind_fails_as_unsupported_naming_the_platform() {
  let wayland = unsupported_activation(ApplicationIdKind::DesktopAppId);
  assert!(matches!(wayland, DriverError::Unsupported { .. }));
  assert!(wayland.to_string().contains("Linux Wayland"), "{wayland}");
}

// The window observation (#324) is a fact about the observation, never a

// usability claim: the spellings below are the whole vocabulary a caller

// sees, and none of them names captureability, focus, or readiness.

#[test]

fn activation_window_observation_serializes_with_stable_status_strings() {
  for (observation, status, json) in [
    (
      ActivationWindowObservation::Present {
        reference: WindowRef {
          id: "0x1234".to_string(),
        },
      },
      "window_present",
      serde_json::json!({ "status": "window_present", "reference": { "id": "0x1234" } }),
    ),
    (ActivationWindowObservation::Absent, "window_absent", serde_json::json!({ "status": "window_absent" })),
    (
      ActivationWindowObservation::Unknown {
        reason: "window observation failed".to_string(),
      },
      "window_unknown",
      serde_json::json!({ "status": "window_unknown", "reason": "window observation failed" }),
    ),
  ] {
    assert_eq!(observation.status(), status);

    assert_eq!(serde_json::to_value(&observation).expect("serialize observation"), json);

    let decoded: ActivationWindowObservation = serde_json::from_value(json).expect("deserialize observation");

    assert_eq!(decoded, observation);
  }
}
