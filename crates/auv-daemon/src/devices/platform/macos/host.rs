//! Remote policy adapter for the physical macOS console and signed Aqua host.
//!
//! Only an OS account ID and exact login-session selector cross the daemon's
//! helper IPC. Credential retrieval and locked-session input remain inside it.

use auv::devices::{DeviceEntryErrorReason, UserSession, UserSessionConnectionKind, UserSessionLockState};
use auv_device_helper_macos::HostError;
use auv_driver_macos::device_session::{ObserveError, observe_console};

use super::policy::{ObservedSession, SessionHost};
use super::unix_account::{Account, resolve_uid};

pub(super) struct MacosSessionHost;

impl MacosSessionHost {
  pub(super) fn new() -> Self {
    Self
  }
}

impl SessionHost for MacosSessionHost {
  fn sessions(&self) -> Result<Vec<ObservedSession>, DeviceEntryErrorReason> {
    let Some(console) = current_console_session()? else {
      return Ok(Vec::new());
    };

    let account = resolve_uid(console.uid).map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;

    if account.uid == 0 || account.name != console.session.user {
      return Err(DeviceEntryErrorReason::UnsupportedOsState);
    }

    Ok(vec![ObservedSession {
      public: console.session,
      os_account_id: account.id,
    }])
  }

  async fn verify_pending_credential(
    &self,
    selected: &ObservedSession,
    authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> Result<(), DeviceEntryErrorReason> {
    let account = self.selected_locked_account(selected)?;
    authorize_effect()?;
    auv_device_helper_macos::probe_locked(&account.home, account.uid, &selected.public.selector).map_err(map_host_error)
  }

  async fn verify_ready_credential(
    &self,
    _selected: &ObservedSession,
    authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> Result<(), DeviceEntryErrorReason> {
    // A Ready macOS enrollment already passed the locked Keychain probe. The
    // installed helper retrieves and submits it during the unlock attempt.
    authorize_effect()
  }

  fn unlock_locked(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
    let account = self.selected_locked_account(selected)?;
    // The helper makes its own fresh locked-session observation before and
    // after Keychain retrieval, then reads back the same usable session.
    // Policy performs another independent observation after this returns.
    auv_device_helper_macos::unlock(&account.home, account.uid, &selected.public.selector).map_err(map_host_error)
  }

  fn lock_usable(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
    let account = self.selected_account(selected, UserSessionLockState::Usable)?;
    // The signed Aqua helper rechecks this exact usable console before
    // posting, then independently observes it becoming locked.
    auv_device_helper_macos::lock(&account.home, account.uid, &selected.public.selector).map_err(map_host_error)
  }
}

impl MacosSessionHost {
  fn selected_locked_account(&self, selected: &ObservedSession) -> Result<Account, DeviceEntryErrorReason> {
    self.selected_account(selected, UserSessionLockState::Locked)
  }

  fn selected_account(&self, selected: &ObservedSession, expected_state: UserSessionLockState) -> Result<Account, DeviceEntryErrorReason> {
    let current = self.sessions()?.into_iter().next().ok_or(DeviceEntryErrorReason::StaleSession)?;
    validate_selected_state(selected, &current, expected_state)?;
    let uid =
      selected.os_account_id.strip_prefix("uid:").and_then(|value| value.parse::<u32>().ok()).ok_or(DeviceEntryErrorReason::StaleSession)?;

    let account = resolve_uid(uid).map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;

    if account.uid == 0 || account.id != selected.os_account_id || account.name != selected.public.user {
      return Err(DeviceEntryErrorReason::StaleSession);
    }

    Ok(account)
  }
}

fn validate_selected_state(
  selected: &ObservedSession,
  current: &ObservedSession,
  expected_state: UserSessionLockState,
) -> Result<(), DeviceEntryErrorReason> {
  if selected.public.selector != current.public.selector
    || selected.public.user != current.public.user
    || selected.os_account_id != current.os_account_id
  {
    return Err(DeviceEntryErrorReason::StaleSession);
  }

  if selected.public.lock_state != expected_state || current.public.lock_state != expected_state {
    return Err(DeviceEntryErrorReason::StaleSession);
  }

  Ok(())
}

fn map_host_error(error: HostError) -> DeviceEntryErrorReason {
  match error {
    HostError::StaleSession | HostError::NotLocked | HostError::AlreadyLocked => DeviceEntryErrorReason::StaleSession,
    HostError::OutcomeUnverified | HostError::InputUnavailable => DeviceEntryErrorReason::OutcomeUnverified,
    HostError::InputUnavailableAt(stage) => {
      // NOTICE(device-entry-macos-diagnostics): The installed LaunchDaemon
      // captures stderr in a root-owned file, while the CLI tracing filter
      // suppresses daemon targets. Log only this fixed stage until the local
      // audit can carry private input diagnostics without exposing them via
      // the paired API.
      eprintln!("AUV macOS locked-session input stage: {stage:?}");
      DeviceEntryErrorReason::OutcomeUnverified
    }
    HostError::VaultUnavailable => DeviceEntryErrorReason::Unenrolled,
    HostError::Unavailable | HostError::Unauthorized | HostError::InvalidRequest => DeviceEntryErrorReason::ServiceUnavailable,
  }
}

// Device policy mapping for the physical macOS console observation.

/// One logged-in console identity and the public facts derived from it.
/// The native observation retains UID and the exact session identity for
/// target-local enrollment and same-session readback.
#[derive(Debug)]
struct ConsoleSession {
  session: UserSession,
  uid: u32,
}

/// Observe one physical, already logged-in user through the shared driver
/// parser. This remains a fresh IORegistry read on each call.
fn current_console_session() -> Result<Option<ConsoleSession>, DeviceEntryErrorReason> {
  let observed = observe_console().map_err(map_observation)?;
  Ok(observed.map(|native| {
    let locked = native.is_locked();
    let uid = native.uid();
    let session = UserSession {
      selector: native.selector().to_owned(),
      user: native.user().to_owned(),
      lock_state: if locked {
        UserSessionLockState::Locked
      } else {
        UserSessionLockState::Usable
      },
      connection_kind: UserSessionConnectionKind::Physical,
      seat: Some("console".to_owned()),
    };

    ConsoleSession { session, uid }
  }))
}

fn map_observation(error: ObserveError) -> DeviceEntryErrorReason {
  match error {
    ObserveError::Unavailable => DeviceEntryErrorReason::ServiceUnavailable,
    ObserveError::UnknownState => DeviceEntryErrorReason::UnsupportedOsState,
    ObserveError::Ambiguous => DeviceEntryErrorReason::AmbiguousUser,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn observed(selector: &str, user: &str, id: &str, lock_state: UserSessionLockState) -> ObservedSession {
    ObservedSession {
      public: UserSession {
        selector: selector.to_owned(),
        user: user.to_owned(),
        lock_state,
        connection_kind: UserSessionConnectionKind::Physical,
        seat: Some("console".to_owned()),
      },
      os_account_id: id.to_owned(),
    }
  }

  #[test]
  fn locked_selection_requires_same_login_instance_and_uid() {
    let selected = observed("macos:uuid-a", "neko", "uid:501", UserSessionLockState::Locked);

    assert_eq!(
      validate_selected_state(
        &selected,
        &observed("macos:uuid-a", "neko", "uid:501", UserSessionLockState::Locked),
        UserSessionLockState::Locked
      ),
      Ok(())
    );
    assert_eq!(
      validate_selected_state(
        &selected,
        &observed("macos:uuid-b", "neko", "uid:501", UserSessionLockState::Locked),
        UserSessionLockState::Locked
      ),
      Err(DeviceEntryErrorReason::StaleSession)
    );
    assert_eq!(
      validate_selected_state(
        &selected,
        &observed("macos:uuid-a", "neko", "uid:502", UserSessionLockState::Locked),
        UserSessionLockState::Locked
      ),
      Err(DeviceEntryErrorReason::StaleSession)
    );
    assert_eq!(
      validate_selected_state(
        &selected,
        &observed("macos:uuid-a", "neko", "uid:501", UserSessionLockState::Usable),
        UserSessionLockState::Locked
      ),
      Err(DeviceEntryErrorReason::StaleSession)
    );
  }

  #[test]
  fn lock_selection_requires_same_usable_console() {
    let selected = observed("macos:uuid-a", "neko", "uid:501", UserSessionLockState::Usable);

    assert_eq!(
      validate_selected_state(
        &selected,
        &observed("macos:uuid-a", "neko", "uid:501", UserSessionLockState::Usable),
        UserSessionLockState::Usable
      ),
      Ok(())
    );
    assert_eq!(
      validate_selected_state(
        &selected,
        &observed("macos:uuid-a", "neko", "uid:501", UserSessionLockState::Locked),
        UserSessionLockState::Usable
      ),
      Err(DeviceEntryErrorReason::StaleSession)
    );
    assert_eq!(
      validate_selected_state(
        &selected,
        &observed("macos:uuid-b", "neko", "uid:501", UserSessionLockState::Usable),
        UserSessionLockState::Usable
      ),
      Err(DeviceEntryErrorReason::StaleSession)
    );
    assert_eq!(
      validate_selected_state(
        &selected,
        &observed("macos:uuid-a", "neko", "uid:501", UserSessionLockState::Usable),
        UserSessionLockState::Locked
      ),
      Err(DeviceEntryErrorReason::StaleSession)
    );
  }

  #[test]
  fn helper_outcomes_do_not_infer_credential_rejection() {
    assert_eq!(map_host_error(HostError::VaultUnavailable), DeviceEntryErrorReason::Unenrolled);
    assert_eq!(map_host_error(HostError::InputUnavailable), DeviceEntryErrorReason::OutcomeUnverified);
    assert_eq!(
      map_host_error(HostError::InputUnavailableAt(auv_device_helper_macos::InputFailure::FocusUnavailable)),
      DeviceEntryErrorReason::OutcomeUnverified
    );
    assert_eq!(map_host_error(HostError::StaleSession), DeviceEntryErrorReason::StaleSession);
  }

  #[test]
  fn maps_observer_ambiguity_and_unknown_state_to_device_errors() {
    assert_eq!(map_observation(ObserveError::Ambiguous), DeviceEntryErrorReason::AmbiguousUser);
    assert_eq!(map_observation(ObserveError::UnknownState), DeviceEntryErrorReason::UnsupportedOsState);
    assert_eq!(map_observation(ObserveError::Unavailable), DeviceEntryErrorReason::ServiceUnavailable);
  }
}
