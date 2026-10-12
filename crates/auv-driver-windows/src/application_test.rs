use super::*;

#[test]
fn activation_verification_distinguishes_verified_foreground_from_activation_only() {
  assert_eq!(
    activation_verification("editor.exe", Ok(Some("editor.exe".to_string()))),
    ApplicationActivationVerification::VerifiedForeground {
      observed_application_id: "editor.exe".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Ok(Some("Editor.EXE".to_string()))),
    ApplicationActivationVerification::VerifiedForeground {
      observed_application_id: "Editor.EXE".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Ok(Some("other.exe".to_string()))),
    ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id: "other.exe".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Ok(None)),
    ApplicationActivationVerification::Unavailable {
      reason: "foreground window observation did not identify an owning process name".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Err("observation denied".to_string())),
    ApplicationActivationVerification::Unavailable {
      reason: "observation denied".to_string(),
    }
  );
}
