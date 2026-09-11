use super::*;

#[test]
fn verification_serializes_the_same_canonical_status_reported_to_humans() {
  for verification in [
    ApplicationActivationVerification::VerifiedForeground {
      observed_bundle_id: "com.example.Editor".to_string(),
    },
    ApplicationActivationVerification::ForegroundMismatch {
      observed_bundle_id: "com.example.Other".to_string(),
    },
    ApplicationActivationVerification::Unavailable {
      reason: "observation unavailable".to_string(),
    },
  ] {
    let serialized = serde_json::to_value(&verification).expect("activation verification should serialize");
    assert_eq!(serialized["status"], verification.status());
  }
}

#[test]
fn process_verification_serializes_the_same_canonical_status_reported_to_humans() {
  for verification in [
    ProcessActivationVerification::VerifiedForeground {
      observed_process: "editor.exe".to_string(),
    },
    ProcessActivationVerification::ForegroundMismatch {
      observed_process: "other.exe".to_string(),
    },
    ProcessActivationVerification::Unavailable {
      reason: "observation unavailable".to_string(),
    },
  ] {
    let serialized = serde_json::to_value(&verification).expect("activation verification should serialize");
    assert_eq!(serialized["status"], verification.status());
  }
}
