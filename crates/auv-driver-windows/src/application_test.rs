use super::*;

#[test]
fn activation_verification_distinguishes_verified_foreground_from_activation_only() {
  assert_eq!(
    activation_verification("editor.exe", Ok(Some("editor.exe".to_string()))),
    ProcessActivationVerification::VerifiedForeground {
      observed_process: "editor.exe".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Ok(Some("Editor.EXE".to_string()))),
    ProcessActivationVerification::VerifiedForeground {
      observed_process: "Editor.EXE".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Ok(Some("other.exe".to_string()))),
    ProcessActivationVerification::ForegroundMismatch {
      observed_process: "other.exe".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Ok(None)),
    ProcessActivationVerification::Unavailable {
      reason: "foreground window observation did not identify an owning process name".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Err("observation denied".to_string())),
    ProcessActivationVerification::Unavailable {
      reason: "observation denied".to_string(),
    }
  );
}
