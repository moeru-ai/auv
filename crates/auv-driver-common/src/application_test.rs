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
    ApplicationIdKind::WmClass,
  ] {
    assert_eq!(serde_json::to_value(kind).expect("serialize"), kind.as_str());
  }
  for method in [
    ApplicationActivationMethod::AppleScriptActivate,
    ApplicationActivationMethod::ForegroundWindow,
    ApplicationActivationMethod::EwmhActiveWindow,
  ] {
    assert_eq!(serde_json::to_value(method).expect("serialize"), method.as_str());
  }
}

#[test]
fn linux_application_id_kinds_fail_as_unsupported_naming_the_platform() {
  let wayland = unsupported_activation(ApplicationIdKind::DesktopAppId);
  assert!(matches!(wayland, DriverError::Unsupported { .. }));
  assert!(wayland.to_string().contains("Linux Wayland"), "{wayland}");

  let x11 = unsupported_activation(ApplicationIdKind::WmClass);
  assert!(matches!(x11, DriverError::Unsupported { .. }));
  assert!(x11.to_string().contains("Linux X11"), "{x11}");
}
