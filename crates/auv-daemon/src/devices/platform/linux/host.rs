//! Same-UID GNOME host for one existing physical login session.
//!
//! NOTICE(device-entry-linux-gate): The same-UID locked PAM and paired
//! DeviceService route passed one supervised gate on `neko-gpu-1`; this is a
//! configuration-specific result, not generic Linux or release-install proof.

use auv::devices::{DeviceEntryErrorReason, UserSession, UserSessionConnectionKind, UserSessionLockState};
use auv_driver_linux::device_unlock::{self, GnomeSession, LockState, UnlockError, UnlockOutcome};

use super::pam_native;
use super::policy::{ObservedSession, SessionHost};
use super::vault_linux::{GnomeSecretVault, VaultError};

pub(super) struct LinuxSessionHost;

impl LinuxSessionHost {
  pub(super) fn new() -> Self {
    Self
  }

  fn selected_locked(&self, selected: &ObservedSession) -> Result<GnomeSession, DeviceEntryErrorReason> {
    selected_in_state_from(selected, unique_sessions(current_user_sessions()?)?, UserSessionLockState::Locked)
  }

  fn selected_usable(&self, selected: &ObservedSession) -> Result<GnomeSession, DeviceEntryErrorReason> {
    selected_in_state_from(selected, unique_sessions(current_user_sessions()?)?, UserSessionLockState::Usable)
  }

  async fn verify_stored_password(
    &self,
    selected: &ObservedSession,
    authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> Result<(), DeviceEntryErrorReason> {
    let session = self.selected_locked(selected)?;
    // logind does not consume password bytes. Revalidate the stored credential
    // against the target's installed GDM PAM policy before every attempt so
    // an OS password change invalidates unattended Device entry.
    let vault = GnomeSecretVault::connect_for_current_user().await.map_err(map_vault_error)?;
    let credential = vault.retrieve(session.uid).await.map_err(map_vault_error)?;
    self.selected_locked(selected)?;
    authorize_effect()?;
    // NOTICE(device-entry-pam-cancellation): Run PAM without an await so a
    // canceled request cannot detach a still-running PAM transaction after
    // the policy/account guards drop. This can block one Tokio worker; move
    // it to a worker only with guard ownership through transaction completion.
    pam_native::verify_password(&session.user, credential.as_slice()).map_err(|error| match error {
      pam_native::VerifyError::Rejected => DeviceEntryErrorReason::CredentialRejected,
      pam_native::VerifyError::Unavailable => DeviceEntryErrorReason::ServiceUnavailable,
    })?;
    self.selected_locked(selected)?;
    Ok(())
  }
}

impl SessionHost for LinuxSessionHost {
  fn sessions(&self) -> Result<Vec<ObservedSession>, DeviceEntryErrorReason> {
    Ok(
      unique_sessions(current_user_sessions()?)?
        .into_iter()
        .map(|current| ObservedSession {
          os_account_id: format!("uid:{}", current.native.uid),
          public: current.session,
        })
        .collect(),
    )
  }

  async fn verify_pending_credential(
    &self,
    selected: &ObservedSession,
    authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> Result<(), DeviceEntryErrorReason> {
    self.verify_stored_password(selected, authorize_effect).await
  }

  async fn verify_ready_credential(
    &self,
    selected: &ObservedSession,
    authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> Result<(), DeviceEntryErrorReason> {
    self.verify_stored_password(selected, authorize_effect).await
  }

  fn unlock_locked(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
    let session = self.selected_locked(selected)?;

    match device_unlock::unlock_user_session(&session).map_err(map_error)? {
      UnlockOutcome::UnlockedExistingSession => Ok(()),
      // The selected session changed after our locked read. Policy must not
      // label that race as a delivered unlock effect.
      UnlockOutcome::AlreadyUsable => Err(DeviceEntryErrorReason::StaleSession),
    }
  }

  fn lock_usable(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
    let session = self.selected_usable(selected)?;
    device_unlock::lock_existing_session(&session).map_err(map_error)
  }
}

fn unique_sessions(sessions: Vec<LinuxSession>) -> Result<Vec<LinuxSession>, DeviceEntryErrorReason> {
  // This installed host gate covers one physical session of the UID. Its
  // Secret Service store is UID-scoped; multi-session selection and effects
  // have not been validated even though logind reports each session's hint.
  // TODO(device-entry-linux-multi-session): Lift this limit only after
  // multi-session routing and same-session effect readback are proven.
  if sessions.len() > 1 {
    return Err(DeviceEntryErrorReason::AmbiguousUser);
  }

  Ok(sessions)
}

fn selected_in_state_from(
  selected: &ObservedSession,
  sessions: Vec<LinuxSession>,
  expected_state: UserSessionLockState,
) -> Result<GnomeSession, DeviceEntryErrorReason> {
  let current =
    sessions.into_iter().find(|current| current.session.selector == selected.public.selector).ok_or(DeviceEntryErrorReason::StaleSession)?;

  if selected.public.lock_state != expected_state
    || current.session != selected.public
    || selected.os_account_id != format!("uid:{}", current.native.uid)
  {
    return Err(DeviceEntryErrorReason::StaleSession);
  }

  Ok(current.native)
}

fn map_vault_error(error: VaultError) -> DeviceEntryErrorReason {
  match error {
    VaultError::Missing | VaultError::InvalidSecret => DeviceEntryErrorReason::Unenrolled,
    VaultError::WrongIdentity => DeviceEntryErrorReason::StaleSession,
    VaultError::Unavailable | VaultError::Locked | VaultError::PromptRequired | VaultError::Ambiguous | VaultError::TimedOut => {
      DeviceEntryErrorReason::ServiceUnavailable
    }
  }
}

// GNOME Wayland host facts for an existing logged-in user session. The driver
// is scoped to the daemon's effective UID; a multi-account host needs an
// authorized per-user worker before the Device policy can expose it.

struct LinuxSession {
  session: UserSession,
  native: GnomeSession,
}

/// Takes one inventory snapshot under the installed host's OS identity.
fn current_user_sessions() -> Result<Vec<LinuxSession>, DeviceEntryErrorReason> {
  let sessions = device_unlock::list_user_sessions()
    .map_err(map_error)?
    .into_iter()
    .map(|native| {
      let session = UserSession {
        selector: native.selector(),
        user: native.user.clone(),
        lock_state: match native.lock_state {
          LockState::Locked => UserSessionLockState::Locked,
          LockState::Usable => UserSessionLockState::Usable,
          LockState::Unknown => UserSessionLockState::Unknown,
        },
        connection_kind: UserSessionConnectionKind::Physical,
        seat: Some(native.seat.clone()),
      };
      LinuxSession { session, native }
    })
    .collect();
  Ok(sessions)
}

fn map_error(error: UnlockError) -> DeviceEntryErrorReason {
  match error {
    UnlockError::ServiceUnavailable => DeviceEntryErrorReason::ServiceUnavailable,
    UnlockError::StaleSession => DeviceEntryErrorReason::StaleSession,
    UnlockError::UnsupportedOsState => DeviceEntryErrorReason::UnsupportedOsState,
    UnlockError::OutcomeUnverified => DeviceEntryErrorReason::OutcomeUnverified,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn session(id: &str, started_at_micros: u64, seat: &str, lock_state: UserSessionLockState) -> LinuxSession {
    let native = GnomeSession {
      id: id.into(),
      started_at_micros,
      user: "neko".into(),
      uid: 1000,
      seat: seat.into(),
      lock_state: match lock_state {
        UserSessionLockState::Locked => LockState::Locked,
        UserSessionLockState::Usable => LockState::Usable,
        UserSessionLockState::Unknown => LockState::Unknown,
      },
    };
    LinuxSession {
      session: UserSession {
        selector: native.selector(),
        user: native.user.clone(),
        lock_state,
        connection_kind: UserSessionConnectionKind::Physical,
        seat: Some(native.seat.clone()),
      },
      native,
    }
  }

  fn selected(current: &LinuxSession) -> ObservedSession {
    ObservedSession {
      public: current.session.clone(),
      os_account_id: "uid:1000".into(),
    }
  }

  #[test]
  fn rejects_multiple_same_uid_sessions_even_for_an_explicit_selector() {
    let one = session("52", 1000, "seat0", UserSessionLockState::Locked);
    let two = session("53", 2000, "seat1", UserSessionLockState::Locked);

    assert!(matches!(unique_sessions(vec![one, two]), Err(DeviceEntryErrorReason::AmbiguousUser)));
  }

  #[test]
  fn selected_locked_requires_exact_login_account_seat_and_state() {
    let current = session("52", 1000, "seat0", UserSessionLockState::Locked);
    let selected = selected(&current);
    let native = current.native.clone();

    assert_eq!(selected_in_state_from(&selected, vec![current], UserSessionLockState::Locked), Ok(native));
    assert!(matches!(
      selected_in_state_from(&selected, vec![session("52", 2000, "seat0", UserSessionLockState::Locked)], UserSessionLockState::Locked),
      Err(DeviceEntryErrorReason::StaleSession)
    ));

    assert!(matches!(
      selected_in_state_from(&selected, vec![session("52", 1000, "seat1", UserSessionLockState::Locked)], UserSessionLockState::Locked),
      Err(DeviceEntryErrorReason::StaleSession)
    ));

    assert!(matches!(
      selected_in_state_from(&selected, vec![session("52", 1000, "seat0", UserSessionLockState::Usable)], UserSessionLockState::Locked),
      Err(DeviceEntryErrorReason::StaleSession)
    ));

    assert!(matches!(
      selected_in_state_from(
        &ObservedSession {
          os_account_id: "uid:1001".into(),
          ..selected
        },
        vec![session("52", 1000, "seat0", UserSessionLockState::Locked)],
        UserSessionLockState::Locked
      ),
      Err(DeviceEntryErrorReason::StaleSession)
    ));
  }

  #[test]
  fn lock_selection_requires_the_exact_usable_login() {
    let current = session("52", 1000, "seat0", UserSessionLockState::Usable);
    let selected = selected(&current);
    let expected = current.native.clone();

    assert_eq!(selected_in_state_from(&selected, vec![current], UserSessionLockState::Usable), Ok(expected));
    assert_eq!(
      selected_in_state_from(&selected, vec![session("52", 2000, "seat0", UserSessionLockState::Usable)], UserSessionLockState::Usable),
      Err(DeviceEntryErrorReason::StaleSession)
    );
    assert_eq!(
      selected_in_state_from(&selected, vec![session("52", 1000, "seat0", UserSessionLockState::Locked)], UserSessionLockState::Usable),
      Err(DeviceEntryErrorReason::StaleSession)
    );
    assert_eq!(
      selected_in_state_from(&selected, vec![session("52", 1000, "seat0", UserSessionLockState::Usable)], UserSessionLockState::Locked),
      Err(DeviceEntryErrorReason::StaleSession)
    );
  }

  #[test]
  fn missing_or_unreadable_vault_item_never_means_ready() {
    assert_eq!(map_vault_error(VaultError::Missing), DeviceEntryErrorReason::Unenrolled);
    assert_eq!(map_vault_error(VaultError::Locked), DeviceEntryErrorReason::ServiceUnavailable);
    assert_eq!(map_vault_error(VaultError::PromptRequired), DeviceEntryErrorReason::ServiceUnavailable);
  }

  /// Read-only installed gate: run this one test under the intended daemon UID
  /// after the owner locks its existing GNOME session. The selector is public
  /// session identity, never a credential; the test emits no secret bytes.
  #[tokio::test]
  #[ignore = "requires one enrolled, already locked GNOME session on the installed host"]
  async fn installed_locked_secret_retrieval_without_unlock() {
    let expected = std::env::var("AUV_LINUX_LOCKED_GATE_SELECTOR").expect("set the exact non-secret locked session selector");
    let host = LinuxSessionHost::new();
    let mut before = host.sessions().unwrap();

    assert_eq!(before.len(), 1);

    let selected = before.remove(0);

    assert_eq!(selected.public.selector, expected);
    assert_eq!(selected.public.lock_state, UserSessionLockState::Locked);

    host.verify_pending_credential(&selected, &|| Ok(())).await.unwrap();

    let mut after = host.sessions().unwrap();

    assert_eq!(after.len(), 1);

    let current = after.remove(0);

    assert_eq!(current.public, selected.public);
    assert_eq!(current.os_account_id, selected.os_account_id);
  }

  /// Installed effect gate. The test exercises the same host and vault path
  /// as policy, but logind reports a requested unlock, not visible UI success.
  /// A supervising owner must confirm the desktop after this one attempt.
  #[tokio::test]
  #[ignore = "requires owner-supervised locked GNOME session and sends one logind Unlock"]
  async fn installed_locked_enrollment_unlock_once() {
    let expected = std::env::var("AUV_LINUX_UNLOCK_GATE_SELECTOR").expect("set the exact non-secret locked session selector");
    let host = LinuxSessionHost::new();
    let mut before = host.sessions().unwrap();

    assert_eq!(before.len(), 1);

    let selected = before.remove(0);

    assert_eq!(selected.public.selector, expected);
    assert_eq!(selected.public.lock_state, UserSessionLockState::Locked);

    host.verify_pending_credential(&selected, &|| Ok(())).await.unwrap();
    host.unlock_locked(&selected).unwrap();

    let mut after = host.sessions().unwrap();

    assert_eq!(after.len(), 1);

    let current = after.remove(0);

    assert_eq!(current.public.selector, selected.public.selector);
    assert_eq!(current.public.lock_state, UserSessionLockState::Usable);
    assert_eq!(current.os_account_id, selected.os_account_id);
  }
}
