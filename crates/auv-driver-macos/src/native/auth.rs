#[cfg(target_os = "macos")]
use super::binding::ffi::{
  NativeHumanApprovalResponse, NativeHumanApprovalStatus as NativeHumanApprovalStatusFfi,
  request_human_approval as native_request_human_approval,
};
use super::types::AuvResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeHumanApprovalStatus {
  Approved,
  Declined,
  TimedOut,
  Unavailable,
}

impl NativeHumanApprovalStatus {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Approved => "approved",
      Self::Declined => "declined",
      Self::TimedOut => "timed_out",
      Self::Unavailable => "unavailable",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeHumanApproval {
  pub status: NativeHumanApprovalStatus,
  pub approved_at_unix_ms: Option<u64>,
  pub mechanism: String,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

#[cfg(target_os = "macos")]
pub fn request_human_approval(reason: impl Into<String>, timeout_ms: u64) -> AuvResult<NativeHumanApproval> {
  if timeout_ms == 0 {
    return Err("human approval timeout must be greater than 0".to_string());
  }
  Ok(NativeHumanApproval::from(native_request_human_approval(reason.into(), timeout_ms)))
}

#[cfg(not(target_os = "macos"))]
pub fn request_human_approval(_reason: impl Into<String>, _timeout_ms: u64) -> AuvResult<NativeHumanApproval> {
  Err("macOS native human approval is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
impl From<NativeHumanApprovalResponse> for NativeHumanApproval {
  fn from(value: NativeHumanApprovalResponse) -> Self {
    Self {
      status: match value.status {
        NativeHumanApprovalStatusFfi::Approved => NativeHumanApprovalStatus::Approved,
        NativeHumanApprovalStatusFfi::Declined => NativeHumanApprovalStatus::Declined,
        NativeHumanApprovalStatusFfi::TimedOut => NativeHumanApprovalStatus::TimedOut,
        NativeHumanApprovalStatusFfi::Unavailable => NativeHumanApprovalStatus::Unavailable,
      },
      approved_at_unix_ms: u64::try_from(value.approved_at_unix_ms).ok().filter(|value| *value > 0),
      mechanism: if value.mechanism.trim().is_empty() {
        "unknown".to_string()
      } else {
        value.mechanism
      },
      error_message: value.error_message,
      recovery_hint: value.recovery_hint,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::NativeHumanApprovalStatus;

  #[test]
  fn native_human_approval_status_labels_are_stable() {
    assert_eq!(NativeHumanApprovalStatus::Approved.as_str(), "approved");
    assert_eq!(NativeHumanApprovalStatus::Declined.as_str(), "declined");
    assert_eq!(NativeHumanApprovalStatus::TimedOut.as_str(), "timed_out");
    assert_eq!(NativeHumanApprovalStatus::Unavailable.as_str(), "unavailable");
  }
}
