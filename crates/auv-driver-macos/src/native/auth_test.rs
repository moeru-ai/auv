use super::NativeHumanApprovalStatus;

#[test]
fn native_human_approval_status_labels_are_stable() {
  assert_eq!(NativeHumanApprovalStatus::Approved.as_str(), "approved");
  assert_eq!(NativeHumanApprovalStatus::Declined.as_str(), "declined");
  assert_eq!(NativeHumanApprovalStatus::TimedOut.as_str(), "timed_out");
  assert_eq!(NativeHumanApprovalStatus::Unavailable.as_str(), "unavailable");
}
