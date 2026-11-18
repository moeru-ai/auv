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
      observed_application_id: "com.apple.TextEdit".to_string(),
    }
  );
  assert_eq!(
    activation_verification("com.apple.TextEdit", Ok("com.apple.finder".to_string())),
    ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id: "com.apple.finder".to_string(),
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
fn activation_verification_reports_unavailable_when_no_frontmost_bundle_id_was_observed() {
  assert_eq!(
    activation_verification("com.apple.TextEdit", Ok("  ".to_string())),
    ApplicationActivationVerification::Unavailable {
      reason: "WindowServer observation did not identify a frontmost application bundle id".to_string(),
    }
  );
}

#[test]
fn activation_verification_matches_bundle_ids_case_insensitively() {
  assert_eq!(
    activation_verification("com.apple.textedit", Ok("com.apple.TextEdit".to_string())),
    ApplicationActivationVerification::VerifiedForeground {
      observed_application_id: "com.apple.TextEdit".to_string(),
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
// If application.activate had to launch an application through `open -b` (AppleScript
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
      observed_application_id: "com.apple.calculator".to_string(),
    }
  );
  assert_eq!(
    wait_for_foreground("com.apple.calculator", Duration::ZERO, || Ok("com.apple.finder".to_string())),
    ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id: "com.apple.finder".to_string(),
    }
  );
  // An observation that cannot be made does not keep the caller waiting.
  let started = std::time::Instant::now();
  assert!(matches!(
    wait_for_foreground("com.apple.calculator", Duration::from_secs(5), || Err("denied".to_string())),
    ApplicationActivationVerification::Unavailable { .. }
  ));
  assert!(started.elapsed() < Duration::from_secs(1));
}

fn frontmost_snapshot(bundle_id: &str, live: bool) -> crate::types::ObservedWindowSnapshot {
  crate::types::ObservedWindowSnapshot {
    frontmost_app_name: String::new(),
    frontmost_app_bundle_id: bundle_id.to_string(),
    frontmost_window_title: String::new(),
    frontmost_app_live: live,
    observed_at: "test".to_string(),
    windows: Vec::new(),
  }
}

// https://github.com/moeru-ai/auv/issues/481
//
// ROOT CAUSE:
//
// If Accessibility was not trusted, the window-list snapshot named the
// frontmost application from `NSWorkspace.frontmostApplication`, which a
// process that never pumps its main run loop keeps reporting from its first
// read. application.activate compared that value with the requested bundle id
// and reported verified_foreground while another application was in front.
//
// Before the fix, a stale fallback value matching the request verified the
// activation. The fix accepts only a live frontmost reading as evidence; a
// fallback reading yields activation_only_verification_unavailable at once.
#[test]
fn activation_without_a_live_frontmost_reading_is_unavailable_not_verified_issue_481() {
  let started = std::time::Instant::now();
  let verification = wait_for_foreground("com.google.Chrome", Duration::from_secs(5), || {
    live_frontmost_bundle_id(frontmost_snapshot("com.google.Chrome", false))
  });
  match &verification {
    ApplicationActivationVerification::Unavailable { reason } => {
      assert!(reason.contains("com.google.Chrome") && reason.contains("Accessibility"), "unexpected reason: {reason}");
    }
    other => panic!("a stale fallback reading must not verify activation: {other:?}"),
  }
  assert_eq!(verification.status(), "activation_only_verification_unavailable");
  assert!(started.elapsed() < Duration::from_secs(1), "an unavailable reading must not wait for the budget");

  assert_eq!(
    wait_for_foreground("com.google.Chrome", Duration::ZERO, || live_frontmost_bundle_id(frontmost_snapshot("com.google.Chrome", true))),
    ApplicationActivationVerification::VerifiedForeground {
      observed_application_id: "com.google.Chrome".to_string(),
    }
  );
}
