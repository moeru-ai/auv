//! Lock-shortcut delivery for one existing, usable macOS console session.
//!
//! The installed signed Aqua helper owns this capability. Its caller must
//! independently observe the same session becoming locked after input.

/// Fixed native delivery failure; this carries no credential or UI text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockFailure {
  AlreadyLocked,
  IdentityMismatch,
  PermissionMissing,
  SessionChanged,
  EventUnavailable,
  Unavailable,
}

/// Send Control–Command–Q only while the selected console is still usable.
/// A successful post does not establish that macOS locked the session.
#[cfg(target_os = "macos")]
pub fn submit(expected_uid: u32, selector: &str) -> Result<(), LockFailure> {
  use crate::native::binding::ffi::NativeDeviceLockOutcome as Outcome;

  match crate::native::binding::ffi::lock_selected_session(expected_uid, selector.to_owned()) {
    Outcome::Submitted => Ok(()),
    Outcome::AlreadyLocked => Err(LockFailure::AlreadyLocked),
    Outcome::IdentityMismatch => Err(LockFailure::IdentityMismatch),
    Outcome::PermissionMissing => Err(LockFailure::PermissionMissing),
    Outcome::SessionChanged => Err(LockFailure::SessionChanged),
    Outcome::EventUnavailable => Err(LockFailure::EventUnavailable),
  }
}

#[cfg(not(target_os = "macos"))]
pub fn submit(_expected_uid: u32, _selector: &str) -> Result<(), LockFailure> {
  Err(LockFailure::Unavailable)
}
