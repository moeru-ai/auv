//! Windows physical-console adapter for the shared Device policy.
//!
//! The daemon runs as the logged-in user. Console observation, PIN retrieval,
//! and selected-session input belong to the installed LocalSystem Helper Host;
//! this adapter only selects the login and maps the host's typed results.

use auv::devices::{DeviceEntryErrorReason, UserSession, UserSessionConnectionKind, UserSessionLockState};
use auv_device_helper_windows::{ConsoleLockState, ConsoleSession, HostError};

use super::policy::{ObservedSession, SessionHost};

pub(super) struct WindowsSessionHost;

impl SessionHost for WindowsSessionHost {
  fn sessions(&self) -> Result<Vec<ObservedSession>, DeviceEntryErrorReason> {
    Ok(auv_device_helper_windows::observe().map_err(host_error)?.map(|session| vec![observed(&session)]).unwrap_or_default())
  }

  async fn verify_pending_credential(
    &self,
    selected: &ObservedSession,
    authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> Result<(), DeviceEntryErrorReason> {
    let session = selected_console(selected, ConsoleLockState::Locked)?;
    authorize_effect()?;
    auv_device_helper_windows::probe_locked(&session).map_err(host_error)
  }

  async fn verify_ready_credential(
    &self,
    _selected: &ObservedSession,
    _authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> Result<(), DeviceEntryErrorReason> {
    // Ready means the Helper Host retrieved the PIN in a prior locked
    // attempt. It retrieves and submits it again during delivery.
    // TODO(device-entry-windows-rotation): A confirmed OS rejection signal is
    // needed before suspending a rotated PIN; the worker currently reports an
    // unverified result instead. Reopen after the installed Winlogon gate.
    Ok(())
  }

  fn unlock_locked(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
    let session = selected_console(selected, ConsoleLockState::Locked)?;
    // The host re-observes this exact login before retrieval and reads it
    // back as usable after the worker exits. Policy observes independently.
    auv_device_helper_windows::unlock(&session).map_err(host_error)
  }

  fn lock_usable(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
    let session = selected_console(selected, ConsoleLockState::Usable)?;
    auv_device_helper_windows::lock(&session).map_err(host_error)
  }
}

/// Re-reads the console and returns it only if it is still the selected
/// login, account, and expected lock state in both the selection and the OS.
fn selected_console(selected: &ObservedSession, state: ConsoleLockState) -> Result<ConsoleSession, DeviceEntryErrorReason> {
  let current = auv_device_helper_windows::observe().map_err(host_error)?.ok_or(DeviceEntryErrorReason::StaleSession)?;

  if selected_matches_console(selected, &current, state) {
    Ok(current)
  } else {
    Err(DeviceEntryErrorReason::StaleSession)
  }
}

fn selected_matches_console(selected: &ObservedSession, current: &ConsoleSession, state: ConsoleLockState) -> bool {
  selected.public.selector == current.selector()
    && selected.public.user == current.account_name()
    && selected.os_account_id == current.account_sid
    && selected.public.lock_state == public_lock_state(state)
    && current.lock_state == state
}

fn public_lock_state(state: ConsoleLockState) -> UserSessionLockState {
  match state {
    ConsoleLockState::Locked => UserSessionLockState::Locked,
    ConsoleLockState::Usable => UserSessionLockState::Usable,
    ConsoleLockState::Unknown => UserSessionLockState::Unknown,
  }
}

fn observed(session: &ConsoleSession) -> ObservedSession {
  ObservedSession {
    public: UserSession {
      selector: session.selector(),
      user: session.account_name(),
      lock_state: public_lock_state(session.lock_state),
      connection_kind: UserSessionConnectionKind::Physical,
      seat: Some("console".into()),
    },
    os_account_id: session.account_sid.clone(),
  }
}

pub(super) fn host_error(error: HostError) -> DeviceEntryErrorReason {
  match error {
    HostError::StaleSession | HostError::NotLocked => DeviceEntryErrorReason::StaleSession,
    HostError::Unverified => DeviceEntryErrorReason::OutcomeUnverified,
    HostError::NotEnrolled => DeviceEntryErrorReason::Unenrolled,
    // The console login belongs to an account other than this daemon's user.
    HostError::Unauthorized => DeviceEntryErrorReason::OccupiedDesktop,
    HostError::ProtocolUnsupported => DeviceEntryErrorReason::HostIncompatible,
    HostError::Unavailable
    | HostError::Untrusted
    | HostError::InvalidRequest
    | HostError::InvalidCredential
    | HostError::VaultUnavailable => DeviceEntryErrorReason::ServiceUnavailable,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn session(state: ConsoleLockState) -> ConsoleSession {
    ConsoleSession {
      session_id: 2,
      logon_time: 123,
      account_sid: "S-1-5-21-123-456-789-1001".into(),
      domain: "DESKTOP".into(),
      user: "neko".into(),
      lock_state: state,
    }
  }

  #[test]
  fn selected_login_requires_exact_session_sid_name_and_lock() {
    let current = session(ConsoleLockState::Locked);
    let selected = observed(&current);

    assert_eq!(selected.public.user, r"DESKTOP\neko");
    assert!(selected_matches_console(&selected, &current, ConsoleLockState::Locked));

    let mut different = current.clone();
    different.logon_time += 1;

    assert!(!selected_matches_console(&selected, &different, ConsoleLockState::Locked));

    different = current.clone();
    different.account_sid = "S-1-5-21-123-456-789-1002".into();

    assert!(!selected_matches_console(&selected, &different, ConsoleLockState::Locked));
    assert!(!selected_matches_console(&selected, &session(ConsoleLockState::Usable), ConsoleLockState::Locked));
  }

  #[test]
  fn helper_results_map_to_stable_device_entry_reasons() {
    assert_eq!(host_error(HostError::Unavailable), DeviceEntryErrorReason::ServiceUnavailable);
    assert_eq!(host_error(HostError::Untrusted), DeviceEntryErrorReason::ServiceUnavailable);
    assert_eq!(host_error(HostError::ProtocolUnsupported), DeviceEntryErrorReason::HostIncompatible);
    assert_eq!(host_error(HostError::Unauthorized), DeviceEntryErrorReason::OccupiedDesktop);
    assert_eq!(host_error(HostError::NotEnrolled), DeviceEntryErrorReason::Unenrolled);
    assert_eq!(host_error(HostError::Unverified), DeviceEntryErrorReason::OutcomeUnverified);
  }

  #[test]
  fn lock_requires_the_selected_login_to_still_be_usable() {
    let current = session(ConsoleLockState::Usable);
    let selected = observed(&current);

    assert!(selected_matches_console(&selected, &current, ConsoleLockState::Usable));
    assert!(!selected_matches_console(&selected, &session(ConsoleLockState::Locked), ConsoleLockState::Usable));
    assert!(!selected_matches_console(&observed(&session(ConsoleLockState::Locked)), &current, ConsoleLockState::Usable));
  }
}
