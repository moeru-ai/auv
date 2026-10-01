//! Windows physical-console adapter for the shared Device policy.
//!
//! The installed LocalSystem service delegates exact-session input to its
//! selected-session worker and reads back the console state.

use auv::devices::{DeviceEntryErrorReason, UserSession, UserSessionConnectionKind, UserSessionLockState};
use auv_driver_windows::device_session::{ConsoleLockState, ConsoleSession, ConsoleSessionError, observe_console};
use auv_driver_windows::device_unlock_host::{HostError, unlock_with_worker};

use super::policy::{ObservedSession, SessionHost};
use super::vault_windows::{self, VaultError};

pub(super) struct WindowsSessionHost;

impl SessionHost for WindowsSessionHost {
  fn sessions(&self) -> Result<Vec<ObservedSession>, DeviceEntryErrorReason> {
    Ok(observe_console().map_err(session_error)?.map(|session| vec![observed(&session)]).unwrap_or_default())
  }

  async fn verify_pending_credential(
    &self,
    selected: &ObservedSession,
    authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> Result<(), DeviceEntryErrorReason> {
    let session = selected_console(selected, ConsoleLockState::Locked)?;
    authorize_effect()?;
    vault_windows::verify_while_locked(&session).map_err(vault_error)
  }

  async fn verify_ready_credential(
    &self,
    _selected: &ObservedSession,
    _authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> Result<(), DeviceEntryErrorReason> {
    // Ready means the LocalSystem host retrieved the PIN in a prior locked
    // attempt. The worker retrieves and submits it again during delivery.
    // TODO(device-entry-windows-rotation): A confirmed OS rejection signal is
    // needed before suspending a rotated PIN; the worker currently reports an
    // unverified result instead. Reopen after the installed Winlogon gate.
    Ok(())
  }

  fn unlock_locked(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
    let session = selected_console(selected, ConsoleLockState::Locked)?;
    // Retrieval stays in this LocalSystem service; the driver receives the
    // credential only for the one-shot pipe transfer to its worker.
    let credential = vault_windows::retrieve(&session.account_sid).map_err(vault_error)?;
    let after = unlock_with_worker(&session, &credential).map_err(host_error)?;

    if !session.same_login(&after) || after.lock_state != ConsoleLockState::Usable {
      return Err(DeviceEntryErrorReason::OutcomeUnverified);
    }

    Ok(())
  }
}

/// Re-reads the console and returns it only if it is still the selected
/// login, account, and expected lock state in both the selection and the OS.
fn selected_console(selected: &ObservedSession, state: ConsoleLockState) -> Result<ConsoleSession, DeviceEntryErrorReason> {
  let current = observe_console().map_err(session_error)?.ok_or(DeviceEntryErrorReason::StaleSession)?;

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

fn session_error(error: ConsoleSessionError) -> DeviceEntryErrorReason {
  match error {
    ConsoleSessionError::ConsoleTransition => DeviceEntryErrorReason::StaleSession,
    ConsoleSessionError::InconsistentRecord | ConsoleSessionError::UnsupportedPlatform => DeviceEntryErrorReason::UnsupportedOsState,
    ConsoleSessionError::QueryFailed(_) | ConsoleSessionError::IdentityUnverified => DeviceEntryErrorReason::ServiceUnavailable,
  }
}

fn vault_error(error: VaultError) -> DeviceEntryErrorReason {
  match error {
    VaultError::NotEnrolled => DeviceEntryErrorReason::Unenrolled,
    VaultError::NotLocked | VaultError::InvalidAccount => DeviceEntryErrorReason::StaleSession,
    VaultError::Unavailable | VaultError::Permissions | VaultError::RetrievalFailed => DeviceEntryErrorReason::ServiceUnavailable,
  }
}

fn host_error(error: HostError) -> DeviceEntryErrorReason {
  match error {
    HostError::StaleSession | HostError::NotLocked => DeviceEntryErrorReason::StaleSession,
    HostError::Unverified => DeviceEntryErrorReason::OutcomeUnverified,
    HostError::Unavailable | HostError::WorkerUnavailable | HostError::WorkerIdentity | HostError::TransferFailed => {
      DeviceEntryErrorReason::ServiceUnavailable
    }
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
}
