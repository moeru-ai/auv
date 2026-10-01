//! Credential posting for an existing, locked macOS console session.
//!
//! NOTICE(device-entry-macos-host): This is a native input primitive, not an
//! unlock result. It must be called only by an installed, signed graphical
//! helper after target-local authority, enrollment, and same-session checks.
//! The caller must verify the selected OS session became usable afterward.

/// Fixed input stage returned by the signed graphical host. No credential or
/// native diagnostic text is retained in this value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputFailure {
  InvalidRequest,
  IdentityMismatch,
  PermissionMissing,
  SessionChanged,
  EventUnavailable,
  WakeUnavailable,
  FocusUnavailable,
  FocusLost,
  DeadlineExceeded,
  Unavailable,
}

/// Submit one locally retrieved UTF-8 credential at the HID event tap.
///
/// The credential is never put in a process argument, environment variable,
/// file, diagnostic message, or tracing field here. Native posting has no
/// recipient acknowledgement and cannot itself establish an unlock effect.
#[cfg(target_os = "macos")]
pub fn submit(credential: &[u8], expected_uid: u32, selector: &str, posting_budget_seconds: f64) -> Result<(), InputFailure> {
  use crate::native::binding::ffi::NativeDeviceUnlockOutcome as Outcome;

  // TODO(device-entry-macos-secret-memory): The Swift String and CGEvent copies
  // cannot be reliably zeroized. Keep the helper short-lived; revisit when a
  // secret-safe native event path is validated on the installed host.
  let outcome = crate::native::binding::ffi::submit_locked_session_credential(
    credential.to_vec(),
    expected_uid,
    selector.to_owned(),
    posting_budget_seconds,
  );

  match outcome {
    Outcome::Submitted => Ok(()),
    Outcome::InvalidRequest => Err(InputFailure::InvalidRequest),
    Outcome::IdentityMismatch => Err(InputFailure::IdentityMismatch),
    Outcome::PermissionMissing => Err(InputFailure::PermissionMissing),
    Outcome::SessionChanged => Err(InputFailure::SessionChanged),
    Outcome::EventUnavailable => Err(InputFailure::EventUnavailable),
    Outcome::WakeUnavailable => Err(InputFailure::WakeUnavailable),
    Outcome::FocusUnavailable => Err(InputFailure::FocusUnavailable),
    Outcome::FocusLost => Err(InputFailure::FocusLost),
    Outcome::DeadlineExceeded => Err(InputFailure::DeadlineExceeded),
  }
}

#[cfg(not(target_os = "macos"))]
pub fn submit(_credential: &[u8], _expected_uid: u32, _selector: &str, _posting_budget_seconds: f64) -> Result<(), InputFailure> {
  Err(InputFailure::Unavailable)
}
