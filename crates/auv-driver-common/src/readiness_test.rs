use super::*;

#[test]
fn readiness_report_selects_first_failed_check_as_blocker() {
  let report = ReadinessReport::from_checks(
    vec![
      ReadinessCheck::pass("accessibility", "ok"),
      ReadinessCheck::fail("target_window_present", "missing window"),
    ],
    Some("11".to_string()),
    None,
    None,
  );

  assert_eq!(report.status, ReadinessStatus::NotReady);
  assert_eq!(report.selected_blocker.as_deref(), Some("missing window"));
}
