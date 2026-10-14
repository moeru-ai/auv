use super::*;

#[test]
fn activation_script_is_scoped_to_exact_bundle_id() {
  assert_eq!(
    activation_script("com.apple.TextEdit").expect("script"),
    "with timeout of 10 seconds\ntell application id \"com.apple.TextEdit\" to activate\nend timeout"
  );
}

#[test]
fn activation_script_rejects_blank_bundle_id() {
  let error = activation_script("   ").expect_err("blank bundle id should fail");
  assert!(error.to_string().contains("non-empty bundle id"));
}

#[test]
fn activation_verification_distinguishes_verified_foreground_from_activation_only() {
  assert_eq!(
    activation_verification("com.apple.TextEdit", Ok("com.apple.TextEdit".to_string())),
    ApplicationActivationVerification::VerifiedForeground {
      observed_bundle_id: "com.apple.TextEdit".to_string(),
    }
  );
  assert_eq!(
    activation_verification("com.apple.TextEdit", Ok("com.apple.finder".to_string())),
    ApplicationActivationVerification::ForegroundMismatch {
      observed_bundle_id: "com.apple.finder".to_string(),
    }
  );
  assert_eq!(
    activation_verification("com.apple.TextEdit", Err("observation denied".to_string())),
    ApplicationActivationVerification::Unavailable {
      reason: "observation denied".to_string(),
    }
  );
}

#[test]
fn activation_script_escapes_applescript_string_content() {
  assert!(
    activation_script("com.example.\\\"quoted").expect("script").contains("tell application id \"com.example.\\\\\\\"quoted\" to activate")
  );
}

// ROOT CAUSE:
//
// If app.activate had to launch an application through `open -b` (AppleScript
// cannot launch some apps, such as Calculator: error -600), the single
// frontmost check ran 150 ms later, before the application reached the
// front, so a successful activation reported a foreground mismatch.
//
// The fix keeps re-observing until the requested application is frontmost or
// the verification budget runs out.
#[test]
fn foreground_wait_returns_once_the_launched_application_is_frontmost() {
  let mut observations = [
    "com.apple.finder",
    "com.apple.finder",
    "com.apple.calculator",
  ]
  .into_iter();
  let verification = wait_for_foreground("com.apple.calculator", Duration::from_secs(5), || {
    Ok(observations.next().unwrap_or("com.apple.finder").to_string())
  });
  assert_eq!(
    verification,
    ApplicationActivationVerification::VerifiedForeground {
      observed_bundle_id: "com.apple.calculator".to_string(),
    }
  );
  assert_eq!(
    wait_for_foreground("com.apple.calculator", Duration::ZERO, || Ok("com.apple.finder".to_string())),
    ApplicationActivationVerification::ForegroundMismatch {
      observed_bundle_id: "com.apple.finder".to_string(),
    }
  );
}
